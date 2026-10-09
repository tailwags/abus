// SPDX-License-Identifier: EUPL-1.2
use std::num::NonZero;

use abus::{Connection, Header, Message, MessageType, ObjectPathRef, Uuid};
use anyhow::{Result, bail};
use bytes::Bytes;
use futures_util::SinkExt;
use sap::{Argument, Parser};
use tokio_stream::StreamExt;
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

    if let Some(arg) = parser.forward()? {
        match arg {
            Argument::Short('h') | Argument::Long("help") => {
                usage(&name);
                return Ok(());
            }
            Argument::Value(val) => {
                command = Some(val.to_string());
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

    let mut connection = Connection::new().await?;
    info!(server_guid = %connection.server_guid(), "connected");

    let mut header = Header::new(MessageType::MethodCall, const { NonZero::new(1).unwrap() });
    header
        .set_path(ObjectPathRef::new("/org/freedesktop/DBus")?)
        .set_interface("org.freedesktop.DBus")
        .set_member("Hello")
        .set_destination("org.freedesktop.DBus");

    let hello = Message {
        header,
        body: Bytes::new(),
    };

    connection.send(hello).await?;
    info!("Hello sent, waiting for reply");

    if let Some(msg) = connection.try_next().await? {
        info!("received message {msg:#?}");
        // break;
    }

    Ok(())
}
