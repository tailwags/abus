// SPDX-License-Identifier: Apache-2.0
use std::num::NonZero;

use abus::{Connection, Header, Message, MessageType, Uuid, object_path};
use anyhow::Result;
use bytes::Bytes;
use futures_util::SinkExt;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let uuid = Uuid::new()?;

    dbg!(uuid);

    let mut connection = Connection::system().await?;

    println!("Connected to server {}", connection.server_guid());

    let mut header = Header::new(MessageType::MethodCall, const { NonZero::new(1).unwrap() });
    header
        .set_path(object_path!("/org/freedesktop/DBus"))
        .set_interface("org.freedesktop.DBus")
        .set_member("Hello")
        .set_destination("org.freedesktop.DBus");

    let hello = Message {
        header,
        body: Bytes::new(),
    };

    connection.send(hello).await?;

    while let Some(msg) = connection.try_next().await? {
        dbg!(msg);
    }

    Ok(())
}
