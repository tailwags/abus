use std::num::NonZero;

use abus::{
    Connection, Endianness, Flags, Header, Message, MessageCodec, MessageType, ObjectPath, Uuid,
};
use anyhow::{Result, bail};
use bytes::Bytes;
use futures_util::SinkExt;
use sap::{Argument, Parser};
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;
use tracing::info;

const USAGE: &str = "\
Commands:
  hello    Connect to the system bus and send Hello

Options:
  -h, --help    Print this help message";

fn usage(name: &str) {
    eprintln!("Usage: {name} <command> [options]\n\n{USAGE}");
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let mut parser = Parser::from_env()?;
    let name = parser.name().to_owned();

    let mut command: Option<String> = None;

    while let Some(arg) = parser.forward()? {
        match arg {
            Argument::Short('h') | Argument::Long("help") => {
                usage(&name);
                return Ok(());
            }
            Argument::Value(val) => {
                command = Some(val.to_string());
                break;
            }
            Argument::Short(c) => bail!("Unknown flag: -{c}"),
            Argument::Long(s) => bail!("Unknown flag: --{s}"),
            Argument::Stdio => bail!("Unexpected stdin argument"),
        }
    }

    match command.as_deref() {
        Some("hello") => cmd_hello().await,
        Some(cmd) => {
            eprintln!("Unknown command: {cmd}");
            usage(&name);
            std::process::exit(1);
        }
        None => {
            usage(&name);
            Ok(())
        }
    }
}

async fn cmd_hello() -> Result<()> {
    let uuid = Uuid::new()?;
    info!(?uuid, "generated UUID");

    let connection = Connection::new().await?;
    info!(server_guid = %connection.server_guid(), "connected");

    let mut framed = Framed::new(connection, MessageCodec::new());

    let hello = Message {
        header: Header {
            endianness: Endianness::LittleEndian,
            message_type: MessageType::MethodCall,
            flags: Flags::empty(),
            version: 1,
            body_length: 0,
            serial: const { NonZero::new(1).unwrap() },
            path: Some(ObjectPath::new("/org/freedesktop/DBus".to_owned()).unwrap()),
            interface: Some("org.freedesktop.DBus".to_owned()),
            member: Some("Hello".to_owned()),
            error_name: None,
            reply_serial: None,
            destination: Some("org.freedesktop.DBus".to_owned()),
            sender: None,
            signature: None,
            unix_fds: None,
        },
        body: Bytes::new(),
    };

    framed.send(hello).await?;
    info!("Hello sent, waiting for reply");

    while let Some(msg) = framed.try_next().await? {
        info!(?msg, "received message");
        break;
    }

    Ok(())
}
