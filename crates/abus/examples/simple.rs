use std::num::NonZero;

use abus::{
    Connection, Endianness, Flags, Header, Message, MessageCodec, MessageType, ObjectPath, Uuid,
};
use anyhow::Result;
use bytes::Bytes;
use futures_util::SinkExt;
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let uuid = Uuid::new()?;

    dbg!(uuid);

    let connection = Connection::new().await?;

    println!("Connected to server {}", connection.server_guid());

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

    framed.send(hello).await?;

    while let Some(msg) = framed.try_next().await? {
        dbg!(msg);
    }

    Ok(())
}
