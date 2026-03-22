use std::{
    collections::VecDeque,
    fmt::Display,
    io::{IoSlice, IoSliceMut},
    mem::MaybeUninit,
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::net::UnixStream,
    },
    pin::Pin,
    task::{Context, Poll, ready},
};

use rustix::{
    net::{
        RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
        SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
    },
    process::getuid,
};
use tokio::io::{
    self, AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader, ReadBuf,
    unix::AsyncFd,
};
use crate::tracing::{error, info};

use crate::utils::HexU32;

const MAX_FDS_PER_MSG: usize = 253;

enum State {
    #[allow(unused)]
    WaitingForData,
    WaitingForOK,
    WaitingForReject,
    WaitingForAgreeUnixFD,
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WaitingForData => f.write_str("WaitingForData"),
            Self::WaitingForOK => f.write_str("WaitingForOK"),
            Self::WaitingForReject => f.write_str("WaitingForReject"),
            Self::WaitingForAgreeUnixFD => f.write_str("WaitingForAgreeUnixFD"),
        }
    }
}

pub struct Connection {
    stream: BufReader<StreamWrapper>,
}

impl Connection {
    pub async fn new() -> io::Result<Self> {
        let stream = tokio::net::UnixStream::connect("/run/dbus/system_bus_socket").await?;

        info!("Connected to dbus system socket");

        let stream = StreamWrapper::authenticate(stream.into_std()?).await?;

        Ok(Self { stream })
    }

    pub fn server_guid(&self) -> &str {
        &self.stream.get_ref().server_guid
    }
}

struct StreamWrapper {
    stream: AsyncFd<UnixStream>,
    server_guid: String,
    unix_fd_passing: bool,
    pub decode_fds: VecDeque<OwnedFd>,
    pub encode_fds: VecDeque<OwnedFd>,
}

impl StreamWrapper {
    pub async fn authenticate(stream: UnixStream) -> io::Result<BufReader<Self>> {
        let this = Self {
            stream: AsyncFd::new(stream)?,
            server_guid: String::new(),
            unix_fd_passing: false,
            decode_fds: VecDeque::new(),
            encode_fds: VecDeque::new(),
        };

        this._authenticate().await
    }

    async fn _authenticate(self) -> io::Result<BufReader<Self>> {
        let mut stream = BufReader::new(self);

        let uid = HexU32::new(getuid().as_raw());

        stream.write_all(b"\0").await?;

        stream
            .write_all(format!("AUTH EXTERNAL {uid}\r\n").as_bytes())
            .await?;

        let mut state = State::WaitingForOK;

        let mut line_buffer = String::new();

        loop {
            stream.read_line(&mut line_buffer).await?;
            let line = line_buffer.trim_end();

            let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));

            state = match (&state, cmd) {
                (
                    State::WaitingForData | State::WaitingForOK | State::WaitingForReject,
                    "REJECTED",
                ) => {
                    // We would try other auth methods here if we had any
                    // bail!("Auth rejected, available: {arg}")
                    todo!("Auth rejected, available: {arg}")
                }

                (State::WaitingForData | State::WaitingForOK, "ERROR")
                | (State::WaitingForOK, "DATA") => {
                    stream.write_all(b"CANCEL\r\n").await?;
                    State::WaitingForReject
                }

                (State::WaitingForData | State::WaitingForOK, "OK") => {
                    stream.get_mut().server_guid = arg.to_string();
                    stream.write_all(b"NEGOTIATE_UNIX_FD\r\n").await?;

                    State::WaitingForAgreeUnixFD
                }

                (State::WaitingForData, "DATA") => {
                    /*
                    The only mechanism we implement (EXTERNAL) never enters WaitingForData,
                    since it always produces an initial response that the server accepts
                    immediately with OK. If we somehow get here, we have no mechanism
                    capable of processing a server challenge.
                    */
                    stream
                        .write_all(b"ERROR no mechanism to process challenge\r\n")
                        .await?;

                    state
                }

                (State::WaitingForAgreeUnixFD, "AGREE_UNIX_FD") => {
                    stream.get_mut().unix_fd_passing = true;
                    stream.write_all(b"BEGIN\r\n").await?;
                    break;
                }
                (State::WaitingForAgreeUnixFD, "ERROR") => {
                    stream.get_mut().unix_fd_passing = false;
                    stream.write_all(b"BEGIN\r\n").await?;
                    break;
                }

                // Invalid states
                (State::WaitingForData | State::WaitingForOK, _) => {
                    stream.write_all(b"ERROR\r\n").await?;

                    state
                }
                (State::WaitingForReject | State::WaitingForAgreeUnixFD, _) => {
                    error!("Received invalid data during state {state}");
                    // bail!("Closing stream")
                    todo!("Closing stream")
                }
            };

            line_buffer.clear();
        }

        Ok(stream)
    }

    fn poll_write_impl(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let stream = &mut self.stream;
        let encode_fds = &mut self.encode_fds;

        loop {
            let mut guard = ready!(stream.poll_write_ready(cx))?;

            let send_result = {
                let raw: Vec<BorrowedFd<'_>> = encode_fds.iter().map(|fd| fd.as_fd()).collect();

                let mut cmsg_space =
                    [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS_PER_MSG))];
                let mut ancillary = SendAncillaryBuffer::new(&mut cmsg_space);

                if !raw.is_empty() {
                    ancillary.push(SendAncillaryMessage::ScmRights(&raw));
                }

                guard.try_io(|inner| {
                    sendmsg(
                        inner.get_ref(),
                        bufs,
                        &mut ancillary,
                        SendFlags::DONTWAIT | SendFlags::NOSIGNAL,
                    )
                    .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))
                })
            };

            match send_result {
                Ok(Ok(msg)) => {
                    encode_fds.clear();
                    return Poll::Ready(Ok(msg));
                }
                Ok(Err(err)) => {
                    return Poll::Ready(Err(err));
                }

                Err(_would_block) => continue,
            }
        }
    }
}

impl AsyncRead for StreamWrapper {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        let stream = &mut this.stream;
        let decode_fds = &mut this.decode_fds;

        loop {
            let mut guard = ready!(stream.poll_read_ready(cx))?;

            let mut cmsg_space =
                [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS_PER_MSG))];
            let mut ancillary = RecvAncillaryBuffer::new(&mut cmsg_space);

            let unfilled = buf.initialize_unfilled();

            match guard.try_io(|inner| {
                recvmsg(
                    inner.get_ref(),
                    &mut [IoSliceMut::new(unfilled)],
                    &mut ancillary,
                    RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
                )
                .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))
            }) {
                Ok(Ok(msg)) => {
                    for message in ancillary.drain() {
                        if let RecvAncillaryMessage::ScmRights(fds) = message {
                            for fd in fds {
                                decode_fds.push_back(fd);
                            }
                        }
                    }
                    buf.advance(msg.bytes);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(err)) => return Poll::Ready(Err(err)),
                Err(_would_block) => continue,
            }
        }
    }
}

impl AsyncWrite for StreamWrapper {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.get_mut().poll_write_impl(cx, &[IoSlice::new(buf)])
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.get_mut().poll_write_impl(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut()
            .stream
            .get_ref()
            .shutdown(std::net::Shutdown::Write)?;
        Poll::Ready(Ok(()))
    }
}
