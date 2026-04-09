use std::io;

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::Message;

#[derive(Debug)]
pub struct MessageCodec {}

impl MessageCodec {
    pub const fn new() -> Self {
        Self {}
    }
}

impl Decoder for MessageCodec {
    type Item = Message;

    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let total_size = match Message::peek_frame_size(src)? {
            None => {
                src.reserve(16);
                return Ok(None);
            }
            Some(n) => n,
        };

        // Make sure we have the whole body
        if src.len() < total_size {
            src.reserve(total_size - src.len());
            return Ok(None);
        }

        // We have the full body here, split off so we remove this frame and are free to consume
        // NOTE: split_to here is important, it guarantess that we don't leave garbage in the buffer
        let mut src = src.split_to(total_size);

        Message::decode(&mut src).map(Some)
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = io::Error;

    fn encode(&mut self, msg: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        msg.encode(dst)
    }
}
