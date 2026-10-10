// SPDX-License-Identifier: Apache-2.0
//! Serde serializer for message bodies, driven by the body signature.
//!
//! A Rust type alone cannot say whether a `u32` is a `u` or an `h`, whether a string is an `s`,
//! `o` or `g`, or what the element type of an empty `Vec` is. So the signature decides: each
//! `serialize_*` call reads the next type code and fails with [`Error::SignatureMismatch`] if
//! the value does not fit it. There are no implicit conversions. Serde types that D-Bus has no
//! equivalent for (`i8`, `f32`, `char`, `Option`, enums, ...) fail with [`Error::Unsupported`].
//!
//! | serde | D-Bus |
//! |---|---|
//! | `bool` | `b` |
//! | `u8 i16 u16 i32 i64 u64 f64` | `y n q i x t d` |
//! | `u32` | `u`, or `h` (an index into the message's file descriptors) |
//! | `str` | `s`, `o` or `g` (`o` and `g` are validated first) |
//! | bytes | `ay` |
//! | sequence, tuple | `a` (not a dict), or `(...)` |
//! | struct, tuple struct | `(...)` |
//! | map | `a{...}` |
//! | [`Variant`] | `v` |
//! | newtype struct | its contents |
//!
//! The body itself is a tuple of its top-level types: a tuple or struct value supplies one field
//! per type, `()` is the empty body, and any other value needs a body of exactly one type.

use std::{fmt, mem};

use serde_core::ser::{self, Impossible, Serialize};

use crate::{
    Endianness, ObjectPath, ObjectPathError,
    signature::{self, Signature, SignatureError, alignment, single_end},
    utils::align_up,
};

/// Arrays longer than this many bytes must be rejected.
const MAX_ARRAY_LEN: usize = 1 << 26;
/// A whole message is at most 128 MiB, so a body can be no larger.
const MAX_BODY_LEN: usize = 1 << 27;
/// Maximum nesting of arrays, structs, dict entries and variants combined.
const MAX_DEPTH: u8 = 64;
/// Tuple struct name that marks a [`Variant`] to [`Serializer`].
const VARIANT: &str = "$abus::Variant";

#[derive(Debug)]
pub enum Error {
    /// An error reported by the value being serialized.
    Custom(String),
    /// The value does not fit the signature, or has fewer or more fields than it describes.
    SignatureMismatch,
    /// A serde type that D-Bus cannot represent, such as `i8`, `char` or `Option`.
    Unsupported(&'static str),
    /// A string contains a NUL.
    InvalidString,
    /// A string longer than `u32::MAX` bytes.
    StringTooLong(usize),
    InvalidObjectPath(ObjectPathError),
    InvalidSignature(SignatureError),
    ArrayTooLong(usize),
    /// The body is larger than a 128 MiB message.
    BodyTooLarge(usize),
    /// Containers nested more than 64 deep.
    TooDeep,
}

type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Custom(msg) => f.write_str(msg),
            Self::SignatureMismatch => f.write_str("value does not match the signature"),
            Self::Unsupported(ty) => write!(f, "{ty} has no D-Bus equivalent"),
            Self::InvalidString => f.write_str("string contains a NUL"),
            Self::StringTooLong(n) => write!(f, "string of {n} bytes exceeds the 4 GiB limit"),
            Self::InvalidObjectPath(e) => write!(f, "invalid object path: {e}"),
            Self::InvalidSignature(e) => write!(f, "invalid signature: {e}"),
            Self::ArrayTooLong(n) => write!(f, "array of {n} bytes exceeds the 64 MiB limit"),
            Self::BodyTooLarge(n) => write!(f, "body of {n} bytes exceeds the 128 MiB limit"),
            Self::TooDeep => f.write_str("containers nested more than 64 deep"),
        }
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }
}

impl From<SignatureError> for Error {
    fn from(e: SignatureError) -> Self {
        Self::InvalidSignature(e)
    }
}

impl From<ObjectPathError> for Error {
    fn from(e: ObjectPathError) -> Self {
        Self::InvalidObjectPath(e)
    }
}

/// A value together with the signature it is marshalled as, for `v` in a signature.
#[derive(Debug, Clone, Copy)]
pub struct Variant<'a, T> {
    signature: &'a Signature,
    value: T,
}

impl<'a, T> Variant<'a, T> {
    /// `signature` must be exactly one complete type; serializing fails otherwise.
    pub fn new(signature: &'a Signature, value: T) -> Self {
        Self { signature, value }
    }
}

impl<T: Serialize> Serialize for Variant<'_, T> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use ser::SerializeTupleStruct;

        let mut fields = serializer.serialize_tuple_struct(VARIANT, 2)?;
        fields.serialize_field(self.signature.as_str())?;
        fields.serialize_field(&self.value)?;
        fields.end()
    }
}

/// Serializes a body with the given signature and byte order.
pub fn to_bytes<T: Serialize + ?Sized>(
    value: &T,
    signature: &Signature,
    endianness: Endianness,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut ser = Serializer {
        out: &mut out,
        sig: signature.as_bytes(),
        sig_pos: 0,
        endianness,
        depth: 0,
        top: true,
        variant_sig: false,
    };
    value.serialize(&mut ser)?;
    if ser.sig_pos != ser.sig.len() {
        return Err(Error::SignatureMismatch);
    }
    if out.len() > MAX_BODY_LEN {
        return Err(Error::BodyTooLarge(out.len()));
    }
    Ok(out)
}

struct Serializer<'a> {
    /// The body. Offsets in it count from the body start, which is 8-aligned in the message.
    out: &'a mut Vec<u8>,
    /// The signature being walked: the body's, or the one of the current variant.
    sig: &'a [u8],
    /// Position of the next type code in `sig`.
    sig_pos: usize,
    endianness: Endianness,
    depth: u8,
    /// Nothing has been written yet, so a tuple or struct supplies the body's top-level types.
    top: bool,
    /// The next `g` is a variant's signature, so it must be exactly one complete type.
    variant_sig: bool,
}

impl Serializer<'_> {
    /// Consumes the next type code.
    #[inline]
    fn code(&mut self) -> Result<u8> {
        self.top = false;
        let &code = self.sig.get(self.sig_pos).ok_or(Error::SignatureMismatch)?;
        self.sig_pos += 1;
        Ok(code)
    }

    /// Consumes the next type code, which must be `code`.
    #[inline]
    fn expect(&mut self, code: u8) -> Result<()> {
        if self.code()? != code {
            return Err(Error::SignatureMismatch);
        }
        Ok(())
    }

    /// Appends zero padding up to the next multiple of `align`.
    #[inline]
    fn pad(&mut self, align: usize) {
        self.out.resize(align_up(self.out.len(), align), 0);
    }

    #[inline]
    fn u32(&mut self, v: u32) {
        self.pad(4);
        self.endianness.put_u32(self.out, v);
    }

    /// Writes a fixed-size value of type `code`, aligned to its own size.
    #[inline]
    fn fixed<T>(&mut self, code: u8, v: T, put: fn(&Endianness, &mut Vec<u8>, T)) -> Result<()> {
        self.expect(code)?;
        self.pad(mem::size_of::<T>());
        put(&self.endianness, self.out, v);
        Ok(())
    }

    fn string(&mut self, s: &str) -> Result<()> {
        if s.as_bytes().contains(&0) {
            return Err(Error::InvalidString);
        }
        // `to_bytes` rejects bodies over 128 MiB, but the length must fit before that.
        let len = u32::try_from(s.len()).map_err(|_| Error::StringTooLong(s.len()))?;
        self.u32(len);
        self.out.extend_from_slice(s.as_bytes());
        self.out.push(0);
        Ok(())
    }

    fn enter(&mut self) -> Result<()> {
        if self.depth == MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        self.depth += 1;
        Ok(())
    }

    /// Writes a placeholder length and the padding before the first element, which is there
    /// even when the array is empty. `sig_pos` must point at the element type.
    fn array_start(&mut self) -> Result<Kind> {
        self.u32(0);
        let len_at = self.out.len() - 4;
        let elem = self.sig_pos;
        self.pad(alignment(self.sig[elem]));
        self.enter()?;
        Ok(Kind::Array {
            len_at,
            start: self.out.len(),
            elem,
            entry_open: false,
        })
    }

    /// Fills in the length, which excludes the padding before the first element, and moves
    /// past the element type.
    fn array_end(&mut self, len_at: usize, start: usize, elem: usize) -> Result<()> {
        let len = self.out.len() - start;
        if len > MAX_ARRAY_LEN {
            return Err(Error::ArrayTooLong(len));
        }
        let len = self.endianness.u32_to_bytes(len as u32);
        self.out[len_at..len_at + 4].copy_from_slice(&len);
        self.sig_pos = single_end(self.sig, elem);
        self.depth -= 1;
        Ok(())
    }

    fn struct_start(&mut self) -> Result<Kind> {
        self.pad(8);
        self.enter()?;
        Ok(Kind::Struct)
    }

    /// Starts the array or struct a sequence or tuple is serialized as.
    fn seq(&mut self) -> Result<Kind> {
        match self.code()? {
            b'a' if self.sig[self.sig_pos] != b'{' => self.array_start(),
            b'(' => self.struct_start(),
            _ => Err(Error::SignatureMismatch),
        }
    }
}

impl<'s, 'a> ser::Serializer for &'s mut Serializer<'a> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Compound<'s, 'a>;
    type SerializeTuple = Compound<'s, 'a>;
    type SerializeTupleStruct = Compound<'s, 'a>;
    type SerializeTupleVariant = Impossible<(), Error>;
    type SerializeMap = Compound<'s, 'a>;
    type SerializeStruct = Compound<'s, 'a>;
    type SerializeStructVariant = Impossible<(), Error>;

    fn serialize_bool(self, v: bool) -> Result<()> {
        self.expect(b'b')?;
        self.u32(v.into());
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<()> {
        self.expect(b'y')?;
        self.out.push(v);
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<()> {
        self.fixed(b'n', v, Endianness::put_i16)
    }

    fn serialize_u16(self, v: u16) -> Result<()> {
        self.fixed(b'q', v, Endianness::put_u16)
    }

    fn serialize_i32(self, v: i32) -> Result<()> {
        self.fixed(b'i', v, Endianness::put_i32)
    }

    fn serialize_u32(self, v: u32) -> Result<()> {
        if !matches!(self.code()?, b'u' | b'h') {
            return Err(Error::SignatureMismatch);
        }
        self.u32(v);
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<()> {
        self.fixed(b'x', v, Endianness::put_i64)
    }

    fn serialize_u64(self, v: u64) -> Result<()> {
        self.fixed(b't', v, Endianness::put_u64)
    }

    fn serialize_f64(self, v: f64) -> Result<()> {
        self.fixed(b'd', v, Endianness::put_f64)
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        match self.code()? {
            b's' => self.string(v),
            b'o' => {
                ObjectPath::new(v)?;
                self.string(v)
            }
            b'g' => {
                // Validation also limits the length to 255, so it fits the length byte. A
                // variant's signature is held to the stricter single-type rule, which implies
                // everything `validate` checks.
                if mem::take(&mut self.variant_sig) {
                    signature::validate_single_type(v.as_bytes())?;
                } else {
                    signature::validate(v.as_bytes())?;
                }
                self.out.push(v.len() as u8);
                self.out.extend_from_slice(v.as_bytes());
                self.out.push(0);
                Ok(())
            }
            _ => Err(Error::SignatureMismatch),
        }
    }

    /// `ay` is written in one go.
    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.expect(b'a')?;
        if self.sig[self.sig_pos] != b'y' {
            return Err(Error::SignatureMismatch);
        }
        if v.len() > MAX_ARRAY_LEN {
            return Err(Error::ArrayTooLong(v.len()));
        }
        if self.depth == MAX_DEPTH {
            return Err(Error::TooDeep);
        }
        self.sig_pos += 1;
        self.u32(v.len() as u32);
        self.out.extend_from_slice(v);
        Ok(())
    }

    /// The empty body; D-Bus has no unit value otherwise.
    fn serialize_unit(self) -> Result<()> {
        if !mem::take(&mut self.top) {
            return Err(Error::Unsupported("()"));
        }
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        self.serialize_unit()
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Compound<'s, 'a>> {
        let kind = self.seq()?;
        Ok(Compound { ser: self, kind })
    }

    fn serialize_tuple(self, _len: usize) -> Result<Compound<'s, 'a>> {
        let kind = if mem::take(&mut self.top) {
            Kind::Body
        } else {
            self.seq()?
        };
        Ok(Compound { ser: self, kind })
    }

    fn serialize_tuple_struct(self, name: &'static str, len: usize) -> Result<Compound<'s, 'a>> {
        if name == VARIANT {
            self.expect(b'v')?;
            self.enter()?;
            return Ok(Compound {
                ser: self,
                kind: Kind::Variant {
                    sig_at: None,
                    done: false,
                },
            });
        }
        self.serialize_tuple(len)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Compound<'s, 'a>> {
        self.expect(b'a')?;
        if self.sig[self.sig_pos] != b'{' {
            return Err(Error::SignatureMismatch);
        }
        let kind = self.array_start()?;
        Ok(Compound { ser: self, kind })
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Compound<'s, 'a>> {
        let kind = if mem::take(&mut self.top) {
            Kind::Body
        } else {
            self.expect(b'(')?;
            self.struct_start()?
        };
        Ok(Compound { ser: self, kind })
    }

    // D-Bus has no equivalent for these.

    fn serialize_i8(self, _: i8) -> Result<()> {
        Err(Error::Unsupported("i8"))
    }

    fn serialize_i128(self, _: i128) -> Result<()> {
        Err(Error::Unsupported("i128"))
    }

    fn serialize_u128(self, _: u128) -> Result<()> {
        Err(Error::Unsupported("u128"))
    }

    fn serialize_f32(self, _: f32) -> Result<()> {
        Err(Error::Unsupported("f32"))
    }

    fn serialize_char(self, _: char) -> Result<()> {
        Err(Error::Unsupported("char"))
    }

    fn serialize_none(self) -> Result<()> {
        Err(Error::Unsupported("Option"))
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<()> {
        Err(Error::Unsupported("Option"))
    }

    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<()> {
        Err(Error::Unsupported("enum"))
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<()> {
        Err(Error::Unsupported("enum"))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(Error::Unsupported("enum"))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(Error::Unsupported("enum"))
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

enum Kind {
    /// Elements of an array, or entries of a dict. `len_at` is where the length goes, `start`
    /// where the first element starts, and `elem` the element type's position in the signature.
    /// `entry_open` is set between a dict entry's key and its value.
    Array {
        len_at: usize,
        start: usize,
        elem: usize,
        entry_open: bool,
    },
    /// Fields of a struct, up to the closing `)`.
    Struct,
    /// The top-level types of the body.
    Body,
    /// A [`Variant`]: first its signature, then its value. `sig_at` is where the signature
    /// was written, once it has been, and `done` whether the value has been written.
    Variant { sig_at: Option<usize>, done: bool },
}

/// Serializes the parts of a container.
struct Compound<'s, 'a> {
    ser: &'s mut Serializer<'a>,
    kind: Kind,
}

impl Compound<'_, '_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        let ser = &mut *self.ser;
        match self.kind {
            Kind::Array { elem, .. } => {
                ser.sig_pos = elem;
                value.serialize(ser)
            }
            Kind::Struct | Kind::Body => match ser.sig.get(ser.sig_pos) {
                None | Some(b')') => Err(Error::SignatureMismatch),
                Some(_) => value.serialize(ser),
            },
            Kind::Variant { sig_at: None, .. } => {
                // Write the signature as a `g`, which `serialize_str` checks is a single
                // complete type.
                let at = ser.out.len();
                let outer_sig = mem::replace(&mut ser.sig, &b"g"[..]);
                let outer_pos = mem::replace(&mut ser.sig_pos, 0);
                ser.variant_sig = true;
                let result = value.serialize(&mut *ser);
                // Only a validated `g` consumes the type code and succeeds.
                let written = ser.sig_pos == 1;
                ser.sig = outer_sig;
                ser.sig_pos = outer_pos;
                ser.variant_sig = false;
                result?;
                if !written {
                    return Err(Error::SignatureMismatch);
                }
                self.kind = Kind::Variant {
                    sig_at: Some(at),
                    done: false,
                };
                Ok(())
            }
            Kind::Variant { done: true, .. } => Err(Error::SignatureMismatch),
            Kind::Variant {
                sig_at: Some(at), ..
            } => {
                // Walk the value with the variant's signature. It is copied out of the body
                // because the body keeps growing while it is borrowed.
                let len = ser.out[at] as usize;
                let mut sig = [0; 255];
                sig[..len].copy_from_slice(&ser.out[at + 1..][..len]);
                let mut inner = Serializer {
                    out: &mut *ser.out,
                    sig: &sig[..len],
                    sig_pos: 0,
                    endianness: ser.endianness,
                    depth: ser.depth,
                    top: false,
                    variant_sig: false,
                };
                value.serialize(&mut inner)?;
                if inner.sig_pos != len {
                    return Err(Error::SignatureMismatch);
                }
                self.kind = Kind::Variant {
                    sig_at: Some(at),
                    done: true,
                };
                Ok(())
            }
        }
    }

    fn finish(self) -> Result<()> {
        let ser = self.ser;
        match self.kind {
            // A key without its value.
            Kind::Array {
                entry_open: true, ..
            } => Err(Error::SignatureMismatch),
            Kind::Array {
                len_at,
                start,
                elem,
                entry_open: false,
            } => ser.array_end(len_at, start, elem),
            Kind::Struct => {
                // Every field must have been written.
                if ser.sig.get(ser.sig_pos) != Some(&b')') {
                    return Err(Error::SignatureMismatch);
                }
                ser.sig_pos += 1;
                ser.depth -= 1;
                Ok(())
            }
            Kind::Body => Ok(()),
            Kind::Variant { done: true, .. } => {
                ser.depth -= 1;
                Ok(())
            }
            // The signature or the value is missing.
            Kind::Variant { done: false, .. } => Err(Error::SignatureMismatch),
        }
    }
}

impl ser::SerializeSeq for Compound<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.element(value)
    }

    fn end(self) -> Result<()> {
        self.finish()
    }
}

impl ser::SerializeTuple for Compound<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.element(value)
    }

    fn end(self) -> Result<()> {
        self.finish()
    }
}

impl ser::SerializeTupleStruct for Compound<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.element(value)
    }

    fn end(self) -> Result<()> {
        self.finish()
    }
}

impl ser::SerializeStruct for Compound<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<()> {
        self.element(value)
    }

    fn end(self) -> Result<()> {
        self.finish()
    }
}

/// Dict entries are written as key/value pairs inside the `{` and `}` of the element type.
impl ser::SerializeMap for Compound<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<()> {
        let Kind::Array {
            elem,
            ref mut entry_open,
            ..
        } = self.kind
        else {
            return Err(Error::SignatureMismatch);
        };
        // A second key before the value of the first.
        if *entry_open {
            return Err(Error::SignatureMismatch);
        }
        let ser = &mut *self.ser;
        ser.sig_pos = elem + 1;
        ser.pad(8);
        ser.enter()?;
        *entry_open = true;
        key.serialize(ser)
    }

    /// Fails instead of leaving the entry's depth unbalanced if no key came first.
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        let Kind::Array {
            entry_open: ref mut open @ true,
            ..
        } = self.kind
        else {
            return Err(Error::SignatureMismatch);
        };
        *open = false;
        let result = value.serialize(&mut *self.ser);
        self.ser.depth -= 1;
        result
    }

    fn end(self) -> Result<()> {
        self.finish()
    }
}
