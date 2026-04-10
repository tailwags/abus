// SPDX-License-Identifier: Apache-2.0
use std::num::NonZero;

use abus::{Connection, Endianness, Flags, Header, Message, MessageType, ObjectPath, Uuid};
use anyhow::Result;
use bytes::Bytes;
use futures_util::SinkExt;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let uuid = Uuid::new()?;

    dbg!(uuid);

    let mut connection = Connection::new().await?;

    println!("Connected to server {}", connection.server_guid());

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
            member: Some("Hello".to_string()),
            error_name: None,
            reply_serial: None,
            destination: Some("org.freedesktop.DBus".to_string()),
            sender: None,
            signature: None,
            unix_fds: None,
        },
        body: Bytes::new(),
    };

    connection.send(hello).await?;

    while let Some(msg) = connection.try_next().await? {
        dbg!(msg);
    }

    Ok(())
}
