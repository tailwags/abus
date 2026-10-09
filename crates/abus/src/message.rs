// SPDX-License-Identifier: Apache-2.0
use std::{fmt, num::NonZero};

use bitflags::bitflags;
use bytes::{BufMut, Bytes, BytesMut};
use tokio::io;
use tokio_util::codec::{Decoder, Encoder};

use crate::{Endianness, ObjectPathRef};

#[derive(Debug)]
pub struct Message {
    pub header: Header,
    pub body: Bytes,
}

/// A message header.
///
/// The string fields (path, interface, member, ...) are not stored as separate allocations:
/// they are byte ranges into one shared buffer. For a decoded message that buffer is the raw
/// header itself, split off the connection's read buffer, so decoding allocates nothing.
/// Read them through the accessor methods and change them through the `set_*` methods.
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
    /// The serial number of the message this message is a reply to.
    ///
    /// This header field is controlled by the message sender.
    pub reply_serial: Option<u32>,
    /// The number of Unix file descriptors that accompany the message.
    /// If omitted, it is assumed that no Unix file descriptors accompany the message.
    /// The actual file descriptors need to be transferred via platform specific mechanism out-of-band.
    /// They must be sent at the same time as part of the message itself.
    /// They may not be sent before the first byte of the message itself is transferred or after the last byte of the message itself.
    ///
    /// This header field is controlled by the message sender.
    pub unix_fds: Option<NonZero<u32>>,

    /// Backing storage for the string fields. Append-only, so every `Span` below stays valid.
    strings: BytesMut,
    path: Option<Span>,
    interface: Option<Span>,
    member: Option<Span>,
    error_name: Option<Span>,
    destination: Option<Span>,
    sender: Option<Span>,
    signature: Option<Span>,
}

/// Byte range of a string field inside [`Header::strings`]. Create it with [`Span::new`], or
/// [`Span::new_unchecked`] when the bounds are already known, and read it with [`Span::start`]
/// and [`Span::len`]. Nothing else should touch the fields.
///
/// # Layout
///
/// `Header` holds seven `Option<Span>`s, so the size matters. Offsets fit in `u32` because a
/// message is at most 128 MiB. If one of the fields can never be zero, `Option` can use zero as
/// `None` instead of adding a tag, which brings `Option<Span>` from 12 bytes down to 8.
///
/// The start offset is not always non-zero, though. In a decoded header it is: `strings` is the
/// raw header, and every string comes after the 16-byte fixed part. But a header built with
/// [`Header::new`] starts with empty storage, so the first `set_*` call stores its string at
/// offset 0. So the start is stored plus one. Reserving byte 0 with a filler byte would keep
/// real offsets, but measured about 7 ns slower per built message.
#[derive(Debug, Clone, Copy)]
struct Span {
    /// The start offset plus one, so that it is never zero.
    start_plus_one: NonZero<u32>,
    len: u32,
}

const _: () = assert!(size_of::<Option<Span>>() == 8);

impl Span {
    /// Returns `None` if the range does not fit: `start` must be below `u32::MAX` and `len` at
    /// most `u32::MAX`.
    #[inline(always)]
    const fn new(start: usize, len: usize) -> Option<Self> {
        if start < u32::MAX as usize && len <= u32::MAX as usize {
            // SAFETY: both bounds were just checked.
            Some(unsafe { Self::new_unchecked(start, len) })
        } else {
            None
        }
    }

    /// # Safety
    ///
    /// `start` must be below `u32::MAX` and `len` at most `u32::MAX`.
    #[inline(always)]
    const unsafe fn new_unchecked(start: usize, len: usize) -> Self {
        Self {
            // SAFETY: `start < u32::MAX`, so adding one neither overflows nor gives zero.
            start_plus_one: unsafe { NonZero::new_unchecked(start as u32 + 1) },
            len: len as u32,
        }
    }

    #[inline(always)]
    const fn start(self) -> usize {
        self.start_plus_one.get() as usize - 1
    }

    #[inline(always)]
    const fn len(self) -> usize {
        self.len as usize
    }
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

impl Header {
    /// Creates a header with no fields set, in native byte order, protocol version 1 and no flags.
    pub fn new(message_type: MessageType, serial: NonZero<u32>) -> Self {
        Self {
            endianness: Endianness::NATIVE,
            message_type,
            flags: Flags::empty(),
            version: 1,
            body_length: 0,
            serial,
            reply_serial: None,
            unix_fds: None,
            strings: BytesMut::new(),
            path: None,
            interface: None,
            member: None,
            error_name: None,
            destination: None,
            sender: None,
            signature: None,
        }
    }

    fn get(&self, span: Option<Span>) -> Option<&str> {
        let span = span?;
        let bytes = &self.strings[span.start()..][..span.len()];
        // SAFETY: spans are only created by `Message::decode`, after UTF-8 validation, and by
        // `push`, from a `&str`. `strings` is append-only, so the range still holds those bytes.
        Some(unsafe { std::str::from_utf8_unchecked(bytes) })
    }

    /// Appends `s` to the backing storage and returns its span.
    ///
    /// # Panics
    ///
    /// If the storage grows past `u32::MAX` bytes. Such a header could not be encoded anyway.
    fn push(&mut self, s: &str) -> Span {
        let span =
            Span::new(self.strings.len(), s.len()).expect("header strings exceed u32::MAX bytes");
        self.strings.extend_from_slice(s.as_bytes());
        span
    }

    /// The object to send a call to, or the object a signal is emitted from.
    /// The special path /org/freedesktop/DBus/Local is reserved;
    /// implementations should not send messages with this path,
    /// and the reference implementation of the bus daemon will disconnect any application that attempts to do so.
    ///
    /// This header field is controlled by the message sender.
    pub fn path(&self) -> Option<&ObjectPathRef> {
        // SAFETY: the path span is only set from a validated object path, in `Message::decode`
        // or in `set_path`.
        self.get(self.path)
            .map(|s| unsafe { ObjectPathRef::new_unchecked(s) })
    }

    /// The interface to invoke a method call on, or that a signal is emitted from.
    /// Optional for method calls, required for signals.
    /// The special interface org.freedesktop.DBus.Local is reserved;
    /// implementations should not send messages with this interface,
    /// and the reference implementation of the bus daemon will disconnect any application that attempts to do so.
    ///
    /// This header field is controlled by the message sender.
    pub fn interface(&self) -> Option<&str> {
        self.get(self.interface)
    }

    /// The member, either the method name or signal name. This header field is controlled by the message sender.
    pub fn member(&self) -> Option<&str> {
        self.get(self.member)
    }

    /// The name of the error that occurred, for errors
    pub fn error_name(&self) -> Option<&str> {
        self.get(self.error_name)
    }

    /// The name of the connection this message is intended for.
    /// This field is usually only meaningful in combination with the message bus,
    /// but other servers may define their own meanings for it.
    ///
    /// This header field is controlled by the message sender.
    pub fn destination(&self) -> Option<&str> {
        self.get(self.destination)
    }

    /// Unique name of the sending connection.
    /// This field is usually only meaningful in combination with the message bus,
    /// but other servers may define their own meanings for it.
    ///
    /// On a message bus, this header field is controlled by the message bus,
    /// so it is as reliable and trustworthy as the message bus itself.
    /// Otherwise, this header field is controlled by the message sender,
    /// unless there is out-of-band information that indicates otherwise.
    pub fn sender(&self) -> Option<&str> {
        self.get(self.sender)
    }

    /// The signature of the message body.
    /// If omitted, it is assumed to be the empty signature "" (i.e. the body must be 0-length).
    ///
    /// This header field is controlled by the message sender.
    pub fn signature(&self) -> Option<&str> {
        // FIXME: should be a parsed signature
        self.get(self.signature)
    }

    /// Sets the [path](Self::path) field.
    pub fn set_path(&mut self, path: &ObjectPathRef) -> &mut Self {
        self.path = Some(self.push(path.as_str()));
        self
    }

    /// Sets the [interface](Self::interface) field.
    pub fn set_interface(&mut self, interface: &str) -> &mut Self {
        self.interface = Some(self.push(interface));
        self
    }

    /// Sets the [member](Self::member) field.
    pub fn set_member(&mut self, member: &str) -> &mut Self {
        self.member = Some(self.push(member));
        self
    }

    /// Sets the [error name](Self::error_name) field.
    pub fn set_error_name(&mut self, error_name: &str) -> &mut Self {
        self.error_name = Some(self.push(error_name));
        self
    }

    /// Sets the [destination](Self::destination) field.
    pub fn set_destination(&mut self, destination: &str) -> &mut Self {
        self.destination = Some(self.push(destination));
        self
    }

    /// Sets the [sender](Self::sender) field.
    pub fn set_sender(&mut self, sender: &str) -> &mut Self {
        self.sender = Some(self.push(sender));
        self
    }

    /// Sets the [signature](Self::signature) field.
    pub fn set_signature(&mut self, signature: &str) -> &mut Self {
        self.signature = Some(self.push(signature));
        self
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("endianness", &self.endianness)
            .field("message_type", &self.message_type)
            .field("flags", &self.flags)
            .field("version", &self.version)
            .field("body_length", &self.body_length)
            .field("serial", &self.serial)
            .field("path", &self.path())
            .field("interface", &self.interface())
            .field("member", &self.member())
            .field("error_name", &self.error_name())
            .field("reply_serial", &self.reply_serial)
            .field("destination", &self.destination())
            .field("sender", &self.sender())
            .field("signature", &self.signature())
            .field("unix_fds", &self.unix_fds)
            .finish()
    }
}

impl Message {
    pub fn decode(src: &mut BytesMut) -> io::Result<Self> {
        let (mut header, header_size) = Self::parse(src)?;

        // Spans are offsets from the start of the frame, so the whole header (fixed part and
        // padding included) becomes the string storage. This shares the buffer instead of copying.
        header.strings = src.split_to(header_size);

        let body = src.split_to(header.body_length as usize).freeze();

        Ok(Message { header, body })
    }

    /// Like [`decode`](Self::decode), for a buffer that holds exactly one frame. Splits once
    /// instead of twice, which saves refcount operations on the shared read buffer.
    pub(crate) fn decode_frame(mut frame: BytesMut) -> io::Result<Self> {
        let (mut header, header_size) = Self::parse(&frame)?;

        let body = frame.split_off(header_size).freeze();
        header.strings = frame;

        Ok(Message { header, body })
    }

    /// Parses and validates the header of the frame at the start of `src` without consuming
    /// it. Returns the header, with empty string storage, and the header size.
    #[inline(always)]
    fn parse(src: &[u8]) -> io::Result<(Header, usize)> {
        let total_size = peek_frame_size(src)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "incomplete header"))?;
        if src.len() < total_size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete message",
            ));
        }

        let endianness: Endianness = src[0]
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid endianness"))?;
        let message_type: MessageType = src[1]
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid message type"))?;
        let mut header = Header::new(
            message_type,
            NonZero::new(endianness.u32_from_bytes([src[8], src[9], src[10], src[11]]))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "message serial must not be zero",
                    )
                })?,
        );
        header.endianness = endianness;
        header.flags = Flags::from_bits_retain(src[2]);
        header.version = src[3];
        header.body_length = endianness.u32_from_bytes([src[4], src[5], src[6], src[7]]);

        let array_len = endianness.u32_from_bytes([src[12], src[13], src[14], src[15]]) as usize;
        let array_end = 16 + array_len;
        let header_size = (array_end + 7) & !7;

        // `peek_frame_size` checked that the whole frame is present, so the field array is too.
        // The reader is bounded to the array, so no field can run into the padding or the body.
        let mut reader = FieldReader {
            buf: &src[..array_end],
            pos: 16,
            endianness,
        };

        loop {
            // Each field is STRUCT(BYTE, VARIANT), structs align to 8.
            // Whatever is left after aligning is the array's trailing padding.
            reader.pos = (reader.pos + 7) & !7;
            if reader.pos >= array_end {
                break;
            }

            // After field_code we're at 8k+1. The variant sig is always
            // sig_len(1) + sig(1) + null(1) = 3 bytes, landing at 8k+4 (4-aligned).
            match HeaderField::try_from(reader.u8()?)? {
                HeaderField::Path => {
                    reader.variant_sig(b'o')?;
                    let (span, s) = reader.string()?;
                    ObjectPathRef::new(s)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    header.path = Some(span);
                }

                // All string fields have the same wire shape, just different codes.
                field @ (HeaderField::Interface
                | HeaderField::Member
                | HeaderField::ErrorName
                | HeaderField::Destination
                | HeaderField::Sender) => {
                    reader.variant_sig(b's')?;
                    let (span, _) = reader.string()?;
                    match field {
                        HeaderField::Interface => header.interface = Some(span),
                        HeaderField::Member => header.member = Some(span),
                        HeaderField::ErrorName => header.error_name = Some(span),
                        HeaderField::Destination => header.destination = Some(span),
                        HeaderField::Sender => header.sender = Some(span),
                        // SAFETY: outer `field @` arm already constrains field to the five variants above
                        _ => unsafe { std::hint::unreachable_unchecked() },
                    }
                }

                // SIGNATURE ('g'): u8 length prefix, not u32 like strings.
                HeaderField::Signature => {
                    reader.variant_sig(b'g')?;
                    let (span, _) = reader.signature()?;
                    header.signature = Some(span);
                }

                // REPLY_SERIAL (5) and UNIX_FDS (9) are both u32.
                field @ (HeaderField::ReplySerial | HeaderField::UnixFds) => {
                    reader.variant_sig(b'u')?;
                    let val = reader.u32()?;
                    match field {
                        HeaderField::ReplySerial => header.reply_serial = Some(val),
                        HeaderField::UnixFds => header.unix_fds = NonZero::new(val),
                        // SAFETY: outer `field @` arm already constrains field to ReplySerial and UnixFds
                        _ => unsafe { std::hint::unreachable_unchecked() },
                    }
                }
            }
        }

        Ok((header, header_size))
    }

    pub fn encode(self, dst: &mut BytesMut) -> io::Result<()> {
        let Message { header, body } = self;
        let endianness = header.endianness;

        // `dst` may already hold earlier messages (Framed encodes into a shared write
        // buffer), so every offset and alignment below is relative to `start`.
        let start = dst.len();

        dst.put_u8(endianness.into());
        dst.put_u8(header.message_type.into());
        dst.put_u8(header.flags.bits());
        dst.put_u8(header.version);
        endianness.put_u32(dst, body.len() as u32);
        endianness.put_u32(dst, header.serial.get());

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

        if let Some(path) = header.path() {
            encode_str_field(
                dst,
                start,
                HeaderField::Path,
                b'o',
                path.as_str(),
                endianness,
            );
        }

        if let Some(interface) = header.interface() {
            encode_str_field(
                dst,
                start,
                HeaderField::Interface,
                b's',
                interface,
                endianness,
            );
        }

        if let Some(member) = header.member() {
            encode_str_field(dst, start, HeaderField::Member, b's', member, endianness);
        }

        if let Some(error_name) = header.error_name() {
            encode_str_field(
                dst,
                start,
                HeaderField::ErrorName,
                b's',
                error_name,
                endianness,
            );
        }

        if let Some(reply_serial) = header.reply_serial {
            encode_u32_field(
                dst,
                start,
                HeaderField::ReplySerial,
                reply_serial,
                endianness,
            );
        }

        if let Some(destination) = header.destination() {
            encode_str_field(
                dst,
                start,
                HeaderField::Destination,
                b's',
                destination,
                endianness,
            );
        }

        if let Some(sender) = header.sender() {
            encode_str_field(dst, start, HeaderField::Sender, b's', sender, endianness);
        }

        if let Some(signature) = header.signature() {
            encode_str_field(
                dst,
                start,
                HeaderField::Signature,
                b'g',
                signature,
                endianness,
            );
        }

        if let Some(unix_fds) = header.unix_fds {
            encode_u32_field(dst, start, HeaderField::UnixFds, unix_fds.get(), endianness);
        }

        let array_len = (dst.len() - start - 16) as u32;
        dst[start + 12..start + 16].copy_from_slice(&endianness.u32_to_bytes(array_len));

        // From the spec: "The length of the header must be a multiple of 8, allowing the body to begin on an 8-byte boundary when storing the entire message in a single buffer."
        align_to(dst, start, 8);

        dst.extend_from_slice(&body);

        Ok(())
    }
}

/// Cursor over a message's header field array. Every read is bounds-checked against the end
/// of the array, so malformed input produces an error instead of a panic or an over-read.
///
/// The methods are `#[inline(always)]` because LLVM otherwise sometimes leaves them
/// out of line depending on the caller, which measured about 20 ns slower per message.
struct FieldReader<'a> {
    buf: &'a [u8],
    pos: usize,
    endianness: Endianness,
}

impl<'a> FieldReader<'a> {
    #[inline(always)]
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let bytes = self
            .pos
            .checked_add(n)
            .and_then(|end| self.buf.get(self.pos..end))
            .ok_or_else(past_array_end)?;
        self.pos += n;
        Ok(bytes)
    }

    #[inline(always)]
    fn array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let bytes = *self
            .buf
            .get(self.pos..)
            .and_then(<[u8]>::first_chunk::<N>)
            .ok_or_else(past_array_end)?;
        self.pos += N;
        Ok(bytes)
    }

    #[inline(always)]
    fn u8(&mut self) -> io::Result<u8> {
        let [b] = self.array()?;
        Ok(b)
    }

    #[inline(always)]
    fn u32(&mut self) -> io::Result<u32> {
        let bytes = self.array()?;
        Ok(self.endianness.u32_from_bytes(bytes))
    }

    /// Reads and validates the 3-byte variant type header: sig_len=1, `expected_sig`, null terminator.
    #[inline(always)]
    fn variant_sig(&mut self, expected_sig: u8) -> io::Result<()> {
        let [len, sig, nul] = self.array()?;
        if len != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected variant signature length 1",
            ));
        }
        if sig != expected_sig {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected variant signature byte",
            ));
        }
        if nul != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected null terminator after signature",
            ));
        }
        Ok(())
    }

    /// Reads a u32-length-prefixed string followed by a null terminator.
    /// Used for D-Bus types `s` (STRING) and `o` (OBJECT_PATH).
    #[inline(always)]
    fn string(&mut self) -> io::Result<(Span, &'a str)> {
        let len = self.u32()? as usize;
        self.str_body(len)
    }

    /// Reads a u8-length-prefixed string followed by a null terminator.
    /// Used for D-Bus type `g` (SIGNATURE).
    #[inline(always)]
    fn signature(&mut self) -> io::Result<(Span, &'a str)> {
        let len = self.u8()? as usize;
        self.str_body(len)
    }

    #[inline(always)]
    fn str_body(&mut self, len: usize) -> io::Result<(Span, &'a str)> {
        let start = self.pos;
        let bytes = self.take(len)?;
        if self.u8()? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected null terminator after string",
            ));
        }
        let s = std::str::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // SAFETY: `take` succeeded, so `start + len <= self.buf.len()`. `buf` is part of a frame
        // of at most 128 MiB (checked by `peek_frame_size`), well within what `Span` can hold.
        let span = unsafe { Span::new_unchecked(start, len) };
        Ok((span, s))
    }
}

#[inline]
fn past_array_end() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "header field extends past array boundary",
    )
}

/// Appends nul bytes to `dst` until the length of the message starting at offset `start`
/// is a multiple of `align`. Passing an already-aligned length does nothing. `align` must
/// be a power of two (every alignment D-Bus uses is: 1, 2, 4, or 8).
#[inline]
fn align_to(dst: &mut BytesMut, start: usize, align: usize) {
    debug_assert!(align.is_power_of_two());
    // round the message length up to the next multiple of align, subtract to get how many bytes we need
    let len = dst.len() - start;
    dst.put_bytes(0, ((len + align - 1) & !(align - 1)) - len);
}

/// Encodes a string-like header field (types `'s'`, `'o'`, or `'g'`) into `dst`.
/// Path uses sig `b'o'`, signature field uses `b'g'` with a u8 length prefix;
/// all other string fields use `b's'`.
fn encode_str_field(
    dst: &mut BytesMut,
    start: usize,
    field: HeaderField,
    sig: u8,
    s: &str,
    endianness: Endianness,
) {
    align_to(dst, start, 8);
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
fn encode_u32_field(
    dst: &mut BytesMut,
    start: usize,
    field: HeaderField,
    val: u32,
    endianness: Endianness,
) {
    align_to(dst, start, 8);
    dst.put_u8(field as u8);
    dst.put_u8(1); // signature len
    dst.put_u8(b'u');
    dst.put_u8(0); // null byte to end signature
    endianness.put_u32(dst, val);
}

/// Peeks at `src` to determine the total byte length of the next complete message frame.
///
/// Returns `Ok(None)` if fewer than 16 bytes are available (need more data),
/// `Err` for detectably invalid content (bad endianness byte, frame exceeds 128 MiB),
/// or `Ok(Some(n))` with the total frame size.
fn peek_frame_size(src: &[u8]) -> io::Result<Option<usize>> {
    if src.len() < 16 {
        return Ok(None);
    }
    let endianness = Endianness::try_from(src[0])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid endianness byte"))?;
    let body_length = endianness.u32_from_bytes([src[4], src[5], src[6], src[7]]) as usize;
    let array_len = endianness.u32_from_bytes([src[12], src[13], src[14], src[15]]) as usize;
    let header_size = (16 + array_len + 7) & !7;
    let total_size = header_size + body_length;
    if total_size > 134_217_728 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message exceeds 128 MiB limit",
        ));
    }
    Ok(Some(total_size))
}

#[derive(Debug)]
pub(crate) struct MessageCodec {}

impl Default for MessageCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageCodec {
    pub const fn new() -> Self {
        Self {}
    }
}

impl Decoder for MessageCodec {
    type Item = Message;

    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let total_size = match peek_frame_size(src)? {
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
        let frame = src.split_to(total_size);

        Message::decode_frame(frame).map(Some)
    }
}

impl Encoder<Message> for MessageCodec {
    type Error = io::Error;

    fn encode(&mut self, msg: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        msg.encode(dst)
    }
}
