// SPDX-License-Identifier: Apache-2.0
use bytes::{Buf, BufMut};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
#[repr(u8)]
pub enum Endianness {
    LittleEndian = b'l',
    BigEndian = b'B',
}

impl From<Endianness> for u8 {
    fn from(value: Endianness) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for Endianness {
    type Error = u8;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            b'l' => Ok(Endianness::LittleEndian),
            b'B' => Ok(Endianness::BigEndian),
            _ => Err(value),
        }
    }
}

macro_rules! endianness {
    ($ty:ty, $get:ident, $get_le:ident, $put:ident, $put_le:ident, $from_bytes:ident, $to_bytes:ident) => {
        pub(crate) const fn $from_bytes(&self, bytes: [u8; std::mem::size_of::<$ty>()]) -> $ty {
            match self {
                Endianness::LittleEndian => <$ty>::from_le_bytes(bytes),
                Endianness::BigEndian => <$ty>::from_be_bytes(bytes),
            }
        }

        pub(crate) const fn $to_bytes(&self, val: $ty) -> [u8; std::mem::size_of::<$ty>()] {
            match self {
                Endianness::LittleEndian => val.to_le_bytes(),
                Endianness::BigEndian => val.to_be_bytes(),
            }
        }

        pub(crate) fn $get<B: Buf>(&self, buf: &mut B) -> $ty {
            match self {
                Endianness::LittleEndian => buf.$get_le(),
                Endianness::BigEndian => buf.$get(),
            }
        }

        pub(crate) fn $put<B: BufMut>(&self, buf: &mut B, val: $ty) {
            match self {
                Endianness::LittleEndian => buf.$put_le(val),
                Endianness::BigEndian => buf.$put(val),
            }
        }
    };
}

#[rustfmt::skip]
#[allow(dead_code)]
impl Endianness {
    endianness!(u16, get_u16, get_u16_le, put_u16, put_u16_le, u16_from_bytes, u16_to_bytes);
    endianness!(i16, get_i16, get_i16_le, put_i16, put_i16_le, i16_from_bytes, i16_to_bytes);
    endianness!(u32, get_u32, get_u32_le, put_u32, put_u32_le, u32_from_bytes, u32_to_bytes);
    endianness!(i32, get_i32, get_i32_le, put_i32, put_i32_le, i32_from_bytes, i32_to_bytes);
    endianness!(u64, get_u64, get_u64_le, put_u64, put_u64_le, u64_from_bytes, u64_to_bytes);
    endianness!(i64, get_i64, get_i64_le, put_i64, put_i64_le, i64_from_bytes, i64_to_bytes);
    endianness!(f64, get_f64, get_f64_le, put_f64, put_f64_le, f64_from_bytes, f64_to_bytes);
}
