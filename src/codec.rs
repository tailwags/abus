use std::io;

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::{Endianness, Message};

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
        if src.len() < 16 {
            src.reserve(16);
            return Ok(None);
        }

        let endianness = Endianness::try_from(src[0])
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid endianness byte"))?;

        // Both slices are within the 16-byte minimum guaranteed above.
        let body_length = endianness.u32_from_bytes([src[4], src[5], src[6], src[7]]);
        let array_len = endianness.u32_from_bytes([src[12], src[13], src[14], src[15]]) as usize;

        // 16 fixed header bytes + the array len + padding byte to next multiple of 8
        let header_size = (16 + array_len + 7) & !7;
        let total_size = header_size + body_length as usize;

        /*
        From the spec:

        The maximum length of a message, including header, header alignment padding, and body is 2 to the 27th power or 134217728 (128 MiB).
        Implementations must not send or accept messages exceeding this size.
        */
        if total_size > 134_217_728 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Message exceeds 128 MiB limit",
            ));
        }

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
