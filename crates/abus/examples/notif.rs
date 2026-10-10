// SPDX-License-Identifier: Apache-2.0
//! Sends a desktop notification on the session bus. The same call as:
//!
//! ```sh
//! busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications \
//!   org.freedesktop.Notifications Notify susssasa{sv}i \
//!   "test" 0 "" "Hello" "This is a notification from abus :3" 0 0 5000
//! ```
use std::{collections::HashMap, num::NonZero};

use abus::{
    Connection, Endianness, Header, Message, MessageType, ObjectPath, Signature, Variant,
    object_path, ser,
};
use anyhow::{Result, bail};
use futures_util::SinkExt;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let mut connection = Connection::session().await?;

    // A bus requires Hello before any other call.
    let mut hello = Header::new(MessageType::MethodCall, const { NonZero::new(1).unwrap() });
    hello
        .set_path(object_path!("/org/freedesktop/DBus"))
        .set_interface("org.freedesktop.DBus")
        .set_member("Hello")
        .set_destination("org.freedesktop.DBus");

    connection
        .send(Message {
            header: hello,
            body: Default::default(),
        })
        .await?;

    let reply = wait_for_reply(&mut connection, 1).await?;
    let name: &str = reply.decode_body()?;
    println!("Connected as {name}");

    let signature = Signature::new("susssasa{sv}i")?;
    let body = ser::to_bytes(
        &(
            "test",                                // app_name
            0u32,                                  // replaces_id
            "",                                    // app_icon
            "Hello",                               // summary
            "This is a notification from abus :3", // body
            Vec::<&str>::new(),                    // actions
            HashMap::<&str, Variant<u32>>::new(),  // hints
            5000i32,                               // expire_timeout, in milliseconds
        ),
        signature,
        Endianness::NATIVE,
    )?;

    let mut notify = Header::new(MessageType::MethodCall, const { NonZero::new(2).unwrap() });
    notify
        .set_path(ObjectPath::new("/org/freedesktop/Notifications")?)
        .set_interface("org.freedesktop.Notifications")
        .set_member("Notify")
        .set_destination("org.freedesktop.Notifications")
        .set_signature(signature);

    connection
        .send(Message {
            header: notify,
            body: body.into(),
        })
        .await?;

    let reply = wait_for_reply(&mut connection, 2).await?;
    let id: u32 = reply.decode_body()?;
    println!("Notification id: {id}");

    Ok(())
}

/// Reads messages until the reply to `serial`, skipping everything else (such as the
/// NameAcquired signal the bus sends after Hello).
async fn wait_for_reply(connection: &mut Connection, serial: u32) -> Result<Message> {
    while let Some(message) = connection.try_next().await? {
        if message.header.reply_serial != Some(serial) {
            continue;
        }
        match message.header.message_type {
            MessageType::MethodReturn => return Ok(message),
            MessageType::Error => {
                let name = message.header.error_name().unwrap_or("unknown error");
                let text: &str = message.decode_body().unwrap_or_default();
                bail!("{name}: {text}");
            }
            _ => {}
        }
    }
    bail!("connection closed before the reply to serial {serial}")
}
