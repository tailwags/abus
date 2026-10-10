// SPDX-License-Identifier: Apache-2.0
//! Serde deserializer for message bodies, driven by the body signature.
//!
//! D-Bus data describes itself through its signature, so this works like any self-describing
//! format: each `deserialize_*` call reads the next type code from the signature and passes the
//! value to the visitor's matching `visit_*`. The visitor reports type mismatches.
//!
//! | D-Bus | serde |
//! |---|---|
//! | `y n q i u x t d` | the matching integer or `f64` |
//! | `b` | `bool` |
//! | `h` | `u32`, the index into the message's file descriptors |
//! | `s o g` | borrowed `str` (`o` and `g` are validated first) |
//! | `(...)` | sequence, so tuples and structs |
//! | `a{...}` | map, so maps and structs (keyed by field name) |
//! | other `a` | sequence; `ay` is also borrowed bytes for `&[u8]` |
//! | `v` | the contained value, transparently |
//!
//! The body itself is a tuple of its top-level types: a tuple or struct target gets one field
//! per type. Any other target needs a body of exactly one type and receives that value.
//!
//! The signature is walked in place (see [`Signature`]): nothing is parsed into a table
//! and decoding allocates nothing beyond what the visitor does.

use std::{fmt, mem};

use serde_core::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};

use crate::{
    Endianness, Message, ObjectPath, ObjectPathError,
    cursor::{Cursor, CursorError},
    signature::{self, Signature, SignatureError, alignment, single_end},
};

/// Arrays longer than this many bytes must be rejected.
const MAX_ARRAY_LEN: u32 = 1 << 26;
/// Maximum nesting of arrays, structs, dict entries and variants combined.
const MAX_DEPTH: u8 = 64;

#[derive(Debug)]
pub enum Error {
    /// An error reported by the visitor, such as a type mismatch.
    Custom(String),
    UnexpectedEof,
    NonZeroPadding,
    InvalidBool(u32),
    /// Not UTF-8, contains a NUL, or is not NUL-terminated.
    InvalidString,
    InvalidObjectPath(ObjectPathError),
    InvalidSignature(SignatureError),
    ArrayTooLong(u32),
    /// The array's elements did not end exactly at its declared length.
    ArrayLengthMismatch,
    /// Containers nested more than 64 deep.
    TooDeep,
    /// The target type read fewer or more values than the signature describes.
    SignatureMismatch,
    /// Bytes left in the body after the last value.
    TrailingData,
    /// The target type, named here, has no D-Bus equivalent (such as `i8`, `f32`, `char`,
    /// `Option` or an enum).
    Unsupported(&'static str),
}

type Result<T> = std::result::Result<T, Error>;

/// Implements `deserialize_*` methods that reject a type D-Bus cannot represent.
macro_rules! unsupported {
    ($($method:ident => $ty:literal,)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value> {
                Err(Error::Unsupported($ty))
            }
        )*
    };
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Custom(msg) => f.write_str(msg),
            Self::UnexpectedEof => f.write_str("body ends inside a value"),
            Self::NonZeroPadding => f.write_str("alignment padding is not zero"),
            Self::InvalidBool(n) => write!(f, "invalid boolean value {n}"),
            Self::InvalidString => f.write_str("invalid string"),
            Self::InvalidObjectPath(e) => write!(f, "invalid object path: {e}"),
            Self::InvalidSignature(e) => write!(f, "invalid signature: {e}"),
            Self::ArrayTooLong(n) => write!(f, "array of {n} bytes exceeds the 64 MiB limit"),
            Self::ArrayLengthMismatch => f.write_str("array elements do not match its length"),
            Self::TooDeep => f.write_str("containers nested more than 64 deep"),
            Self::SignatureMismatch => f.write_str("target type does not match the signature"),
            Self::TrailingData => f.write_str("trailing bytes after the body"),
            Self::Unsupported(ty) => write!(f, "{ty} has no D-Bus equivalent"),
        }
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }
}

impl From<SignatureError> for Error {
    fn from(e: SignatureError) -> Self {
        Self::InvalidSignature(e)
    }
}

impl From<CursorError> for Error {
    #[inline]
    fn from(e: CursorError) -> Self {
        match e {
            CursorError::UnexpectedEof => Self::UnexpectedEof,
            CursorError::NonZeroPadding => Self::NonZeroPadding,
            CursorError::InvalidString => Self::InvalidString,
        }
    }
}

impl From<ObjectPathError> for Error {
    fn from(e: ObjectPathError) -> Self {
        Self::InvalidObjectPath(e)
    }
}

impl Message {
    /// Deserializes the body as described by the header's signature.
    pub fn decode_body<'de, T: de::Deserialize<'de>>(&'de self) -> Result<T> {
        let signature = self.header.signature().unwrap_or(Signature::EMPTY);
        from_slice(&self.body, signature, self.header.endianness)
    }
}

/// Deserializes a body with the given signature and byte order.
///
/// `body` must start at an 8-byte boundary of its message, as every body does, because
/// alignment is relative to the start of the message.
pub fn from_slice<'de, T: de::Deserialize<'de>>(
    body: &'de [u8],
    signature: &'de Signature,
    endianness: Endianness,
) -> Result<T> {
    let mut de = Deserializer {
        cur: Cursor::new(body, 0, endianness),
        sig: signature.as_bytes(),
        sig_pos: 0,
        depth: 0,
    };
    let value = T::deserialize(Body(&mut de))?;
    if de.sig_pos != de.sig.len() {
        return Err(Error::SignatureMismatch);
    }
    if de.cur.pos() != body.len() {
        return Err(Error::TrailingData);
    }
    Ok(value)
}

struct Deserializer<'de> {
    /// The body. Offsets count from its start, which is 8-aligned within the message.
    cur: Cursor<'de>,
    /// The signature being walked: the body's, or the one inside the current variant.
    sig: &'de [u8],
    /// Position of the next type code in `sig`.
    sig_pos: usize,
    depth: u8,
}

impl<'de> Deserializer<'de> {
    #[inline]
    fn pos(&self) -> usize {
        self.cur.pos()
    }

    /// Reads a fixed-size basic value. Their alignment equals their size.
    #[inline]
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.cur.aligned()?)
    }

    fn enter(&mut self) -> Result<()> {
        if self.depth == MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        self.depth += 1;
        Ok(())
    }

    /// Reads an array's length and the padding before its first element, which is there even
    /// when the array is empty. `sig_pos` must point at the element type. Returns the element
    /// type's position in the signature and the end of the array data.
    fn array_start(&mut self) -> Result<(usize, usize)> {
        let len = self.cur.u32()?;
        if len > MAX_ARRAY_LEN {
            return Err(Error::ArrayTooLong(len));
        }
        let elem = self.sig_pos;
        self.cur.align(alignment(self.sig[elem]))?;
        self.enter()?;
        let end = self
            .pos()
            .checked_add(len as usize)
            .filter(|&end| end <= self.cur.len())
            .ok_or(Error::UnexpectedEof)?;
        Ok((elem, end))
    }

    /// Checks that the elements ended exactly at `end` and moves past the element type.
    fn array_end(&mut self, elem: usize, end: usize) -> Result<()> {
        if self.pos() != end {
            return Err(Error::ArrayLengthMismatch);
        }
        self.sig_pos = single_end(self.sig, elem);
        self.depth -= 1;
        Ok(())
    }

    /// True if the remaining signature is exactly one complete type.
    fn at_single_type(&self) -> bool {
        self.sig_pos < self.sig.len() && single_end(self.sig, self.sig_pos) == self.sig.len()
    }
}

impl<'de> de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        let &code = self.sig.get(self.sig_pos).ok_or(Error::SignatureMismatch)?;
        self.sig_pos += 1;
        let e = self.cur.endianness();
        match code {
            b'y' => visitor.visit_u8(u8::from_ne_bytes(self.fixed()?)),
            b'b' => match self.cur.u32()? {
                0 => visitor.visit_bool(false),
                1 => visitor.visit_bool(true),
                n => Err(Error::InvalidBool(n)),
            },
            b'n' => visitor.visit_i16(e.i16_from_bytes(self.fixed()?)),
            b'q' => visitor.visit_u16(e.u16_from_bytes(self.fixed()?)),
            b'i' => visitor.visit_i32(e.i32_from_bytes(self.fixed()?)),
            b'u' | b'h' => visitor.visit_u32(self.cur.u32()?),
            b'x' => visitor.visit_i64(e.i64_from_bytes(self.fixed()?)),
            b't' => visitor.visit_u64(e.u64_from_bytes(self.fixed()?)),
            b'd' => visitor.visit_f64(e.f64_from_bytes(self.fixed()?)),
            b's' => visitor.visit_borrowed_str(self.cur.string()?),
            b'o' => {
                let path = self.cur.string()?;
                ObjectPath::new(path)?;
                visitor.visit_borrowed_str(path)
            }
            b'g' => {
                let sig = self.cur.signature()?;
                signature::validate(sig.as_bytes())?;
                visitor.visit_borrowed_str(sig)
            }
            b'v' => {
                // The contained signature is borrowed straight from the body. Swap it in, read
                // the one value it describes, then return to the outer signature.
                let inner = self.cur.signature()?;
                signature::validate_single_type(inner.as_bytes())?;
                self.enter()?;
                let outer_sig = mem::replace(&mut self.sig, inner.as_bytes());
                let outer_pos = mem::replace(&mut self.sig_pos, 0);
                let value = (&mut *self).deserialize_any(visitor)?;
                self.sig = outer_sig;
                self.sig_pos = outer_pos;
                self.depth -= 1;
                Ok(value)
            }
            b'(' => {
                self.cur.align(8)?;
                self.enter()?;
                let value = visitor.visit_seq(Fields(&mut *self))?;
                // Every field must have been read.
                if self.sig.get(self.sig_pos) != Some(&b')') {
                    return Err(Error::SignatureMismatch);
                }
                self.sig_pos += 1;
                self.depth -= 1;
                Ok(value)
            }
            b'a' => {
                let (elem, end) = self.array_start()?;
                let is_dict = self.sig[elem] == b'{';
                let mut elements = Elements {
                    de: &mut *self,
                    elem,
                    end,
                    pending_value: false,
                };
                let value = if is_dict {
                    visitor.visit_map(&mut elements)?
                } else {
                    visitor.visit_seq(&mut elements)?
                };
                self.array_end(elem, end)?;
                Ok(value)
            }
            // Unreachable at a type boundary of a valid signature.
            _ => Err(Error::SignatureMismatch),
        }
    }

    /// `ay` is lent out as one slice of the body; anything else is read as usual.
    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        if self.sig.get(self.sig_pos..self.sig_pos + 2) != Some(b"ay") {
            return self.deserialize_any(visitor);
        }
        self.sig_pos += 1;
        let (elem, end) = self.array_start()?;
        let bytes = self.cur.take(end - self.pos())?;
        self.array_end(elem, end)?;
        visitor.visit_borrowed_bytes(bytes)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn is_human_readable(&self) -> bool {
        false
    }

    unsupported! {
        deserialize_i8 => "i8",
        deserialize_i128 => "i128",
        deserialize_u128 => "u128",
        deserialize_f32 => "f32",
        deserialize_char => "char",
        deserialize_option => "Option",
        deserialize_unit => "()",
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value> {
        Err(Error::Unsupported("unit struct"))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value> {
        Err(Error::Unsupported("enum"))
    }

    serde_core::forward_to_deserialize_any! {
        bool i16 i32 i64 u8 u16 u32 u64 f64 str string
        seq tuple tuple_struct map struct identifier ignored_any
    }
}

/// The fields of a struct, or the top-level types of the body: everything up to the closing
/// `)` or the end of the signature.
struct Fields<'a, 'de>(&'a mut Deserializer<'de>);

impl<'de> SeqAccess<'de> for Fields<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        match self.0.sig.get(self.0.sig_pos) {
            None | Some(b')') => Ok(None),
            Some(_) => seed.deserialize(&mut *self.0).map(Some),
        }
    }
}

/// The elements of an array. Every element restarts at the same element type in the
/// signature; the caller moves past it once the array is done.
struct Elements<'a, 'de> {
    de: &'a mut Deserializer<'de>,
    /// Position of the element type in the signature.
    elem: usize,
    /// End of the array data in the body.
    end: usize,
    /// A dict entry's key was read and its value was not yet. The entry counts towards the
    /// nesting depth until then.
    pending_value: bool,
}

impl<'de> SeqAccess<'de> for Elements<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>> {
        if self.de.pos() >= self.end {
            return Ok(None);
        }
        self.de.sig_pos = self.elem;
        seed.deserialize(&mut *self.de).map(Some)
    }
}

/// Dict entries are read as key/value pairs, skipping the `{` and `}` of the element type.
impl<'de> MapAccess<'de> for Elements<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>> {
        if self.pending_value {
            return Err(de::Error::custom(
                "map key requested before the previous value",
            ));
        }
        if self.de.pos() >= self.end {
            return Ok(None);
        }
        self.de.sig_pos = self.elem + 1;
        self.de.cur.align(8)?;
        self.de.enter()?;
        match seed.deserialize(&mut *self.de) {
            Ok(key) => {
                self.pending_value = true;
                Ok(Some(key))
            }
            Err(e) => {
                self.de.depth -= 1;
                Err(e)
            }
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value> {
        if !self.pending_value {
            return Err(de::Error::custom("map value requested before its key"));
        }
        let value = seed.deserialize(&mut *self.de);
        // `pending_value` means `next_key_seed` entered the dict entry.
        self.pending_value = false;
        self.de.depth -= 1;
        value
    }
}

/// The top level of a body, a tuple of its single complete types.
struct Body<'a, 'de>(&'a mut Deserializer<'de>);

impl<'a, 'de> Body<'a, 'de> {
    /// The deserializer for the body's only value, for targets that are not tuples or structs.
    fn single(self) -> Result<&'a mut Deserializer<'de>> {
        if self.0.at_single_type() {
            Ok(self.0)
        } else {
            Err(Error::SignatureMismatch)
        }
    }
}

impl<'de> de::Deserializer<'de> for Body<'_, 'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.single()?.deserialize_any(visitor)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.single()?.deserialize_bytes(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        self.single()?.deserialize_bytes(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    /// An empty body. `from_slice` rejects a non-empty one, since nothing was read.
    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value> {
        visitor.visit_unit()
    }

    /// An empty body, like `()`.
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_unit()
    }

    unsupported! {
        deserialize_i8 => "i8",
        deserialize_i128 => "i128",
        deserialize_u128 => "u128",
        deserialize_f32 => "f32",
        deserialize_char => "char",
        deserialize_option => "Option",
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value> {
        Err(Error::Unsupported("enum"))
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value> {
        visitor.visit_seq(Fields(self.0))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_seq(Fields(self.0))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value> {
        visitor.visit_seq(Fields(self.0))
    }

    fn is_human_readable(&self) -> bool {
        false
    }

    serde_core::forward_to_deserialize_any! {
        bool i16 i32 i64 u8 u16 u32 u64 f64 str string seq map identifier ignored_any
    }
}
