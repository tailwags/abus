use std::num::NonZero;

use bitflags::bitflags;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::io;

use crate::{Endianness, ObjectPath};

#[derive(Debug)]
pub struct Message {
    pub header: Header,
    pub body: Bytes,
}

#[derive(Debug)]
pub struct Header {
    /// Endianness flag. Both header and body are in this endianness.
    pub endianness: Endianness,
    /// Message type. Unknown types must be ignored.
    pub message_type: MessageType,
    /// Bitwise OR of flags. Unknown flags must be ignored
    pub flags: Flags,
    /// Major protocol version of the sending application.
    /// If the major protocol version of the receiving application does not match,
    /// the applications will not be able to communicate and the D-Bus connection must be disconnected.
    ///
    /// The major protocol version is currently 1 and unlikely to change
    pub version: u8,
    /// Length in bytes of the message body, starting from the end of the header. The header ends after its alignment padding to an 8-boundary.
    pub body_length: u32,
    /// The serial of this message, used as a cookie by the sender to identify the reply corresponding to this request.
    pub serial: NonZero<u32>,
    /// The object to send a call to, or the object a signal is emitted from.
    /// The special path /org/freedesktop/DBus/Local is reserved;
    /// implementations should not send messages with this path,
    /// and the reference implementation of the bus daemon will disconnect any application that attempts to do so.
    ///
    /// This header field is controlled by the message sender.
    pub path: Option<ObjectPath>,
    /// The interface to invoke a method call on, or that a signal is emitted from.
    /// Optional for method calls, required for signals.
    /// The special interface org.freedesktop.DBus.Local is reserved;
    /// implementations should not send messages with this interface,
    /// and the reference implementation of the bus daemon will disconnect any application that attempts to do so.
    ///
    /// This header field is controlled by the message sender.
    pub interface: Option<String>,
    /// The member, either the method name or signal name. This header field is controlled by the message sender.
    pub member: Option<String>,
    /// The name of the error that occurred, for errors
    pub error_name: Option<String>,
    /// The serial number of the message this message is a reply to.
    ///
    /// This header field is controlled by the message sender.
    pub reply_serial: Option<u32>,
    /// The name of the connection this message is intended for.
    /// This field is usually only meaningful in combination with the message bus,
    /// but other servers may define their own meanings for it.
    ///
    /// This header field is controlled by the message sender.
    pub destination: Option<String>,
    /// Unique name of the sending connection.
    /// This field is usually only meaningful in combination with the message bus,
    /// but other servers may define their own meanings for it.
    ///
    /// On a message bus, this header field is controlled by the message bus,
    /// so it is as reliable and trustworthy as the message bus itself.
    /// Otherwise, this header field is controlled by the message sender,
    /// unless there is out-of-band information that indicates otherwise.
    pub sender: Option<String>,
    /// The signature of the message body.
    /// If omitted, it is assumed to be the empty signature "" (i.e. the body must be 0-length).
    ///
    /// This header field is controlled by the message sender.
    pub signature: Option<String>, // FIXME: should be a parser signature
    /// The number of Unix file descriptorsiable and trustworthy as the message bus itself.
    /// Otherwise, this that accompany the message.
    /// If omitted, it is assumed that no Unix file descriptors accompany the message.
    /// The actual file descriptors need to be transferred via platform specific mechanism out-of-band.
    /// They must be sent at the same time as part of the message itself.
    /// They may not be sent before the first byte of the message itself is transferred or after the last byte of the message itself.
    ///
    /// This header field is controlled by the message sender.
    pub unix_fds: Option<NonZero<u32>>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[repr(u8)]
pub enum MessageType {
    /// This is an invalid type.
    Invalid = 0,
    /// Method call. This message type may prompt a reply.
    MethodCall = 1,
    /// Method reply with returned data.
    MethodReturn = 2,
    /// Error reply. If the first argument exists and is a string, it is an error message.
    Error = 3,
    /// Signal emission.
    Signal = 4,
}

impl From<MessageType> for u8 {
    fn from(value: MessageType) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for MessageType {
    type Error = u8;

    fn try_from(value: u8) -> std::result::Result<Self, <Self as TryFrom<u8>>::Error> {
        match value {
            0 => Ok(MessageType::Invalid),
            1 => Ok(MessageType::MethodCall),
            2 => Ok(MessageType::MethodReturn),
            3 => Ok(MessageType::Error),
            4 => Ok(MessageType::Signal),
            _ => Err(value),
        }
    }
}

bitflags! {
    #[derive(Debug)]
    pub struct Flags: u8 {
        /// This message does not expect method return replies or error replies,
        /// even if it is of a type that can have a reply; the reply should be omitted.
        /// Note that METHOD_CALL is the only message type currently defined that can expect a reply,
        /// so the presence or absence of this flag in the other three message types that are currently documented is meaningless:
        /// replies to those message types should not be sent, whether this flag is present or not.
        const NO_REPLY_EXPECTED = 0x1;
        /// The bus must not launch an owner for the destination name in response to this message.
        const NO_AUTO_START = 0x2;
        const ALLOW_INTERACTIVE_AUTHORIZATION = 0x4;

        const _ = !0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HeaderField {
    Path = 1,
    Interface = 2,
    Member = 3,
    ErrorName = 4,
    ReplySerial = 5,
    Destination = 6,
    Sender = 7,
    Signature = 8,
    UnixFds = 9,
}

impl TryFrom<u8> for HeaderField {
    type Error = io::Error;

    fn try_from(value: u8) -> io::Result<Self> {
        match value {
            1 => Ok(HeaderField::Path),
            2 => Ok(HeaderField::Interface),
            3 => Ok(HeaderField::Member),
            4 => Ok(HeaderField::ErrorName),
            5 => Ok(HeaderField::ReplySerial),
            6 => Ok(HeaderField::Destination),
            7 => Ok(HeaderField::Sender),
            8 => Ok(HeaderField::Signature),
            9 => Ok(HeaderField::UnixFds),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown header field code {value}"),
            )),
        }
    }
}

impl Message {
    pub(crate) fn decode(src: &mut BytesMut) -> io::Result<Self> {
        let endianness: Endianness = src
            .get_u8()
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid endianness"))?;
        let message_type: MessageType = src
            .get_u8()
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid message type"))?;
        let flags = Flags::from_bits_retain(src.get_u8());
        let version = src.get_u8();
        let body_length = endianness.get_u32(src);
        let serial = NonZero::new(endianness.get_u32(src)).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "message serial must not be zero",
            )
        })?;
        let array_len = endianness.get_u32(src) as usize;
        let header_size = (16 + array_len + 7) & !7;

        let mut pos = 16usize;
        let array_end = pos + array_len;

        let mut path = None;
        let mut interface = None;
        let mut member = None;
        let mut error_name = None;
        let mut reply_serial = None;
        let mut destination = None;
        let mut sender = None;
        let mut signature = None;
        let mut unix_fds = None;

        while pos < array_end {
            // Each field is STRUCT(BYTE, VARIANT), structs align to 8.
            let pad = (8 - pos % 8) % 8;
            src.advance(pad);
            pos += pad;

            let field_code = src.get_u8();
            pos += 1;

            // After field_code we're at 8k+1. The variant sig is always
            // sig_len(1) + sig(1) + null(1) = 3 bytes, landing at 8k+4 (4-aligned).
            match HeaderField::try_from(field_code)? {
                HeaderField::Path => {
                    read_variant_sig(src, b'o')?;
                    pos += 3;
                    let s = read_string(src, endianness)?;
                    pos += 4 + s.len() + 1;
                    path = Some(ObjectPath { inner: s });
                }

                // All string fields have the same wire shape, just different codes.
                field @ (HeaderField::Interface
                | HeaderField::Member
                | HeaderField::ErrorName
                | HeaderField::Destination
                | HeaderField::Sender) => {
                    read_variant_sig(src, b's')?;
                    pos += 3;
                    let s = read_string(src, endianness)?;
                    pos += 4 + s.len() + 1;
                    match field {
                        HeaderField::Interface => interface = Some(s),
                        HeaderField::Member => member = Some(s),
                        HeaderField::ErrorName => error_name = Some(s),
                        HeaderField::Destination => destination = Some(s),
                        HeaderField::Sender => sender = Some(s),
                        _ => unreachable!(),
                    }
                }

                // SIGNATURE ('g'): u8 length prefix, not u32 like strings.
                HeaderField::Signature => {
                    read_variant_sig(src, b'g')?;
                    pos += 3;
                    let s = read_sig_string(src)?;
                    pos += 1 + s.len() + 1;
                    signature = Some(s);
                }

                // REPLY_SERIAL (5) and UNIX_FDS (9) are both u32.
                field @ (HeaderField::ReplySerial | HeaderField::UnixFds) => {
                    read_variant_sig(src, b'u')?;
                    pos += 3;
                    let val = endianness.get_u32(src);
                    pos += 4;
                    match field {
                        HeaderField::ReplySerial => reply_serial = Some(val),
                        HeaderField::UnixFds => unix_fds = NonZero::new(val),
                        _ => unreachable!(),
                    }
                }
            }
        }

        // Spec: "The length of the header must be a multiple of 8." Advance past array tail padding.
        src.advance(header_size - array_end);

        let body = src.copy_to_bytes(body_length as usize);

        Ok(Message {
            header: Header {
                endianness,
                message_type,
                flags,
                version,
                body_length,
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
            },
            body,
        })
    }

    pub(crate) fn encode(self, dst: &mut BytesMut) -> io::Result<()> {
        let Message { header, body } = self;

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
        endianness.put_u32(dst, body.len() as u32);
        endianness.put_u32(dst, serial.get());

        /*
        The header is up to this point of known size. Next byte will be written at offset 12.
        Offset 12 is where we wanna put the lenght of the array in bytes minus the padding.
        We are putting a 0 there just to advance

        Then we wanna align to the array type alignment, in this case a struct so 8 alignement
        We already know where we are, so we wanna get to the next multiple which is 16.

        In pratice the spec is sorta expecting this so the array len itself makes it aligned

        In pratice we can just put in the u32 for the array len but it's worth nothing why this "just works"
        */

        endianness.put_u32(dst, 0);

        if let Some(ObjectPath { inner: path }) = path {
            encode_str_field(dst, HeaderField::Path, b'o', &path, endianness);
        }

        if let Some(interface) = interface {
            encode_str_field(dst, HeaderField::Interface, b's', &interface, endianness);
        }

        if let Some(member) = member {
            encode_str_field(dst, HeaderField::Member, b's', &member, endianness);
        }

        if let Some(error_name) = error_name {
            encode_str_field(dst, HeaderField::ErrorName, b's', &error_name, endianness);
        }

        if let Some(reply_serial) = reply_serial {
            encode_u32_field(dst, HeaderField::ReplySerial, reply_serial, endianness);
        }

        if let Some(destination) = destination {
            encode_str_field(
                dst,
                HeaderField::Destination,
                b's',
                &destination,
                endianness,
            );
        }

        if let Some(sender) = sender {
            encode_str_field(dst, HeaderField::Sender, b's', &sender, endianness);
        }

        if let Some(signature) = signature {
            encode_str_field(dst, HeaderField::Signature, b'g', &signature, endianness);
        }

        if let Some(unix_fds) = unix_fds {
            encode_u32_field(dst, HeaderField::UnixFds, unix_fds.get(), endianness);
        }

        let array_len = (dst.len() - 16) as u32;
        dst[12..16].copy_from_slice(&endianness.u32_to_bytes(array_len));

        // From the spec: "The length of the header must be a multiple of 8, allowing the body to begin on an 8-byte boundary when storing the entire message in a single buffer."
        align_to(dst, 8);

        dst.extend_from_slice(&body);

        Ok(())
    }
}

/// Reads and validates the 3-byte variant type header: sig_len=1, `expected_sig`, null terminator.
fn read_variant_sig(src: &mut BytesMut, expected_sig: u8) -> io::Result<()> {
    if src.get_u8() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected variant signature length 1",
        ));
    }
    if src.get_u8() != expected_sig {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected variant signature byte",
        ));
    }
    if src.get_u8() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected null terminator after signature",
        ));
    }
    Ok(())
}

/// Reads a u32-length-prefixed string followed by a null terminator.
/// Used for D-Bus types `s` (STRING) and `o` (OBJECT_PATH).
fn read_string(src: &mut BytesMut, endianness: Endianness) -> io::Result<String> {
    let len = endianness.get_u32(src) as usize;
    let bytes = src.copy_to_bytes(len);

    if src.get_u8() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected null terminator after string",
        ));
    }

    String::from_utf8(bytes.to_vec()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Reads a u8-length-prefixed string followed by a null terminator.
/// Used for D-Bus type `g` (SIGNATURE).
fn read_sig_string(src: &mut BytesMut) -> io::Result<String> {
    let len = src.get_u8() as usize;
    let bytes = src.copy_to_bytes(len);

    if src.get_u8() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected null terminator after string",
        ));
    }

    String::from_utf8(bytes.to_vec()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
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
fn encode_str_field(
    dst: &mut BytesMut,
    field: HeaderField,
    sig: u8,
    s: &str,
    endianness: Endianness,
) {
    align_to(dst, 8);
    dst.put_u8(field as u8);
    dst.put_u8(1); // signature len
    dst.put_u8(sig);
    dst.put_u8(0); // null byte to end signature
    if sig == b'g' {
        dst.put_u8(s.len() as u8);
    } else {
        endianness.put_u32(dst, s.len() as u32);
    }
    dst.extend_from_slice(s.as_bytes());
    dst.put_u8(0); // null byte to end string
}

/// Encodes a u32 header field (type `'u'`) into `dst`.
fn encode_u32_field(dst: &mut BytesMut, field: HeaderField, val: u32, endianness: Endianness) {
    align_to(dst, 8);
    dst.put_u8(field as u8);
    dst.put_u8(1); // signature len
    dst.put_u8(b'u');
    dst.put_u8(0); // null byte to end signature
    endianness.put_u32(dst, val);
}
