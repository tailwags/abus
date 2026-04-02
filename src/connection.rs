use std::{
    fmt::Display,
    pin::Pin,
    task::{Context, Poll},
};

use crate::tracing::{error, info};
use anchovy::AnchovyStream;
use pin_project_lite::pin_project;
use rustix::process::getuid;
use tokio::{
    io::{
        self, AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader, ReadBuf,
    },
    net::UnixStream,
};

use crate::utils::HexU32;

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

pin_project! {
    pub struct Connection {
        #[pin]
        stream: BufReader<AnchovyStream>,
        server_guid: String,
        unix_fd_passing: bool,
    }
}

impl Connection {
    pub async fn new() -> io::Result<Self> {
        let stream = UnixStream::connect("/run/dbus/system_bus_socket").await?;

        info!("Connected to dbus system socket");

        Self::authenticate(stream).await
    }

    async fn authenticate(stream: UnixStream) -> io::Result<Self> {
        let mut stream = BufReader::new(AnchovyStream::new(stream)?);

        let uid = HexU32::new(getuid().as_raw());

        stream.write_all(b"\0").await?;

        stream
            .write_all(format!("AUTH EXTERNAL {uid}\r\n").as_bytes())
            .await?;

        let mut state = State::WaitingForOK;
        let mut server_guid = String::new();
        let unix_fd_passing;

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
                    server_guid = arg.to_string();
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
                    unix_fd_passing = true;
                    stream.write_all(b"BEGIN\r\n").await?;
                    break;
                }
                (State::WaitingForAgreeUnixFD, "ERROR") => {
                    unix_fd_passing = false;
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

        Ok(Self {
            stream,
            server_guid,
            unix_fd_passing,
        })
    }

    pub fn server_guid(&self) -> &str {
        &self.server_guid
    }

    pub fn unix_fd_passing(&self) -> bool {
        self.unix_fd_passing
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.project().stream.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.project().stream.poll_shutdown(cx)
    }
}
