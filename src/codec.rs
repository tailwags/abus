use std::io;

use bytes::{BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::{
    Endianness, Message, ObjectPath,
    message::{Header, HeaderField},
};

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

        if !matches!(src[0].try_into(), Ok(Endianness::LittleEndian)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported endianness",
            ));
        }

        // Both slices are within the 16-byte minimum guaranteed above.
        let body_length = u32::from_le_bytes([src[4], src[5], src[6], src[7]]);
        let array_len = u32::from_le_bytes([src[12], src[13], src[14], src[15]]) as usize;

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
        let Message { header, body } = msg;

        // FIXME: abstract away endianess instead of erroring out
        if header.endianness != Endianness::LittleEndian {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported endianess",
            ));
        }

        let Header {
            endianness,
            message_type,
            flags,
            version,
            body_length: _,
            serial,
            path,
            interface,
            member,
            error_name,
            reply_serial,
            destination,
            sender,
            signature,
            unix_fds,
        } = header;

        dst.put_u8(endianness.into());
        dst.put_u8(message_type.into());
        dst.put_u8(flags.bits());
        dst.put_u8(version);
        dst.put_u32_le(body.len() as u32);
        dst.put_u32_le(serial.get());

        /*
        The header is up to this point of known size. Next byte will be written at offset 12.
        Offset 12 is where we wanna put the lenght of the array in bytes minus the padding.
        We are putting a 0 there just to advance

        Then we wanna align to the array type alignment, in this case a struct so 8 alignement
        We already know where we are, so we wanna get to the next multiple which is 16.

        In pratice the spec is sorta expecting this so the array len itself makes it aligned

        In pratice we can just put in the u32 for the array len but it's worth nothing why this "just works"
        */

        dst.put_u32_le(0);

        if let Some(ObjectPath { inner: path }) = path {
            encode_str_field(dst, HeaderField::Path, b'o', &path);
        }

        if let Some(interface) = interface {
            encode_str_field(dst, HeaderField::Interface, b's', &interface);
        }

        if let Some(member) = member {
            encode_str_field(dst, HeaderField::Member, b's', &member);
        }

        if let Some(error_name) = error_name {
            encode_str_field(dst, HeaderField::ErrorName, b's', &error_name);
        }

        if let Some(reply_serial) = reply_serial {
            encode_u32_field(dst, HeaderField::ReplySerial, reply_serial);
        }

        if let Some(destination) = destination {
            encode_str_field(dst, HeaderField::Destination, b's', &destination);
        }

        if let Some(sender) = sender {
            encode_str_field(dst, HeaderField::Sender, b's', &sender);
        }

        if let Some(signature) = signature {
            encode_str_field(dst, HeaderField::Signature, b'g', &signature);
        }

        if let Some(unix_fds) = unix_fds {
            encode_u32_field(dst, HeaderField::UnixFds, unix_fds.get());
        }

        let array_len = (dst.len() - 16) as u32;
        dst[12..16].copy_from_slice(&array_len.to_le_bytes());

        // From the spec: "The length of the header must be a multiple of 8, allowing the body to begin on an 8-byte boundary when storing the entire message in a single buffer."
        align_to(dst, 8);

        dst.extend_from_slice(&body);

        Ok(())
    }
}

/// Appends nul bytes to `dst` until its length is a multiple of `align`.
/// Passing an already-aligned length does nothing. `align` must be a power of two
/// (every alignment D-Bus uses is: 1, 2, 4, or 8).
#[inline]
fn align_to(dst: &mut BytesMut, align: usize) {
    debug_assert!(align.is_power_of_two());
    // round dst.len() up to the next multiple of align, subtract to get how many bytes we need
    dst.put_bytes(0, ((dst.len() + align - 1) & !(align - 1)) - dst.len());
}

/// Encodes a string-like header field (types `'s'`, `'o'`, or `'g'`) into `dst`.
/// Path uses sig `b'o'`, signature field uses `b'g'` with a u8 length prefix;
/// all other string fields use `b's'`.
fn encode_str_field(dst: &mut BytesMut, field: HeaderField, sig: u8, s: &str) {
    align_to(dst, 8);
    dst.put_u8(field as u8);
    dst.put_u8(1); // signature len
    dst.put_u8(sig);
    dst.put_u8(0); // null byte to end signature
    if sig == b'g' {
        dst.put_u8(s.len() as u8);
    } else {
        dst.put_u32_le(s.len() as u32);
    }
    dst.extend_from_slice(s.as_bytes());
    dst.put_u8(0); // null byte to end string
}

/// Encodes a u32 header field (type `'u'`) into `dst`.
fn encode_u32_field(dst: &mut BytesMut, field: HeaderField, val: u32) {
    align_to(dst, 8);
    dst.put_u8(field as u8);
    dst.put_u8(1); // signature len
    dst.put_u8(b'u');
    dst.put_u8(0); // null byte to end signature
    dst.put_u32_le(val);
}
