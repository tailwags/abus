// SPDX-License-Identifier: Apache-2.0
use std::fmt::Display;

use crate::{
    Message,
    codec::MessageCodec,
    tracing::{error, info},
};
use anchovy::{AnchovyStream, DBUS_SCM_RIGHTS};
use futures_util::{Sink, Stream};
use pin_project_lite::pin_project;
use rustix::process::getuid;
use tokio::{
    io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    net::UnixStream,
};
use tokio_util::codec::Framed;

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
        stream: Framed<BufReader<AnchovyStream<DBUS_SCM_RIGHTS>>, MessageCodec>,
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
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("auth rejected, available methods: {arg}"),
                    ));
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
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("received invalid data during state {state}"),
                    ));
                }
            };

            line_buffer.clear();
        }

        Ok(Self {
            stream: Framed::new(stream, MessageCodec::new()),
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

impl Stream for Connection {
    type Item = io::Result<Message>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }
}

impl Sink<Message> for Connection {
    type Error = io::Error;

    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: std::pin::Pin<&mut Self>, msg: Message) -> Result<(), Self::Error> {
        self.project().stream.start_send(msg)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.project().stream.poll_close(cx)
    }
}
