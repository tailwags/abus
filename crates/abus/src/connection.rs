// SPDX-License-Identifier: Apache-2.0
use std::{
    env,
    ffi::OsString,
    fmt::Display,
    os::unix::ffi::OsStringExt as _,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    BUFFER_CAPACITY, Message, MessageCodec,
    tracing::{error, info},
    utils::HexU32,
};
use anchovy::{AnchovyStream, DBUS_FD_LIMIT};
use futures_core::Stream;
use futures_sink::Sink;
use pin_project_lite::pin_project;
use rustix::process::getuid;
use tokio::{
    io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    net::UnixStream,
};
use tokio_util::codec::Framed;

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
        stream: Framed<BufReader<AnchovyStream<DBUS_FD_LIMIT>>, MessageCodec>,
        server_guid: String,
        unix_fd_passing: bool,
    }
}

/// Socket used when `DBUS_SYSTEM_BUS_ADDRESS` is unset. The spec names
/// `/var/run/dbus/system_bus_socket`, which is a symlink to this on modern
/// systems.
const SYSTEM_BUS_PATH: &str = "/run/dbus/system_bus_socket";

/// Reads a D-Bus address from an environment variable. Unset, empty or
/// non-UTF-8 variables yield `None`.
fn env_address(var: &str) -> Option<String> {
    env::var(var).ok().filter(|address| !address.is_empty())
}

/// Returns the socket path of the first `unix:path=` entry of a D-Bus address
/// (see "Server Addresses" in the spec). Other transports and `abstract=`
/// sockets are not supported.
fn unix_path(address: &str) -> io::Result<PathBuf> {
    let value = address
        .split(';')
        .filter_map(|entry| entry.strip_prefix("unix:"))
        .find_map(|params| params.split(',').find_map(|p| p.strip_prefix("path=")))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                format!("dbus address {address:?} has no unix:path= entry"),
            )
        })?;

    Ok(OsString::from_vec(unescape(value)?).into())
}

/// Undoes the `%XX` escaping of address values. Like libdbus, bytes that
/// should have been escaped are accepted as-is.
fn unescape(value: &str) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();

    while let Some(b) = bytes.next() {
        if b != b'%' {
            out.push(b);
            continue;
        }

        let mut hex = || char::from(bytes.next()?).to_digit(16);
        let escaped = hex()
            .zip(hex())
            .map(|(hi, lo)| (hi << 4 | lo) as u8)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid percent escape in dbus address value {value:?}"),
                )
            })?;
        out.push(escaped);
    }

    Ok(out)
}

impl Connection {
    /// Connects to the system bus at `DBUS_SYSTEM_BUS_ADDRESS`, or at the
    /// well-known system bus socket if the variable is unset.
    pub async fn system() -> io::Result<Self> {
        match env_address("DBUS_SYSTEM_BUS_ADDRESS") {
            Some(address) => Self::connect(unix_path(&address)?).await,
            None => Self::connect(SYSTEM_BUS_PATH).await,
        }
    }

    /// Connects to the session bus at `DBUS_SESSION_BUS_ADDRESS`, falling back
    /// to `$XDG_RUNTIME_DIR/bus` like libdbus and sd-bus do.
    pub async fn session() -> io::Result<Self> {
        let path = match env_address("DBUS_SESSION_BUS_ADDRESS") {
            Some(address) => unix_path(&address)?,
            None => env::var_os("XDG_RUNTIME_DIR")
                .filter(|dir| !dir.is_empty())
                .map(|dir| PathBuf::from(dir).join("bus"))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "cannot locate the session bus: neither DBUS_SESSION_BUS_ADDRESS nor XDG_RUNTIME_DIR is set",
                    )
                })?,
        };

        Self::connect(path).await
    }

    /// Connects to the Unix socket at `path` and authenticates.
    pub async fn connect(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let stream = UnixStream::connect(path).await?;

        info!("Connected to dbus socket {}", path.display());

        Self::authenticate(stream).await
    }

    async fn authenticate(stream: UnixStream) -> io::Result<Self> {
        let mut stream = BufReader::with_capacity(BUFFER_CAPACITY, AnchovyStream::new(stream)?);

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
            stream: Framed::with_capacity(stream, MessageCodec::new(), BUFFER_CAPACITY),
            server_guid,
            unix_fd_passing,
        })
    }

    pub fn server_guid(&self) -> &str {
        &self.server_guid
    }

    pub const fn unix_fd_passing(&self) -> bool {
        self.unix_fd_passing
    }
}

impl Stream for Connection {
    type Item = io::Result<Message>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().stream.poll_next(cx)
    }
}

impl Sink<Message> for Connection {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, msg: Message) -> Result<(), Self::Error> {
        self.project().stream.start_send(msg)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_close(cx)
    }
}
