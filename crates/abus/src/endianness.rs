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

#[allow(dead_code)]
impl Endianness {
    pub(crate) fn u32_from_bytes(&self, bytes: [u8; 4]) -> u32 {
        match self {
            Endianness::LittleEndian => u32::from_le_bytes(bytes),
            Endianness::BigEndian => u32::from_be_bytes(bytes),
        }
    }

    pub(crate) fn u32_to_bytes(&self, val: u32) -> [u8; 4] {
        match self {
            Endianness::LittleEndian => val.to_le_bytes(),
            Endianness::BigEndian => val.to_be_bytes(),
        }
    }

    pub(crate) fn get_u16<B: Buf>(&self, buf: &mut B) -> u16 {
        match self {
            Endianness::LittleEndian => buf.get_u16_le(),
            Endianness::BigEndian => buf.get_u16(),
        }
    }

    pub(crate) fn get_i16<B: Buf>(&self, buf: &mut B) -> i16 {
        match self {
            Endianness::LittleEndian => buf.get_i16_le(),
            Endianness::BigEndian => buf.get_i16(),
        }
    }

    pub(crate) fn get_u32<B: Buf>(&self, buf: &mut B) -> u32 {
        match self {
            Endianness::LittleEndian => buf.get_u32_le(),
            Endianness::BigEndian => buf.get_u32(),
        }
    }

    pub(crate) fn get_i32<B: Buf>(&self, buf: &mut B) -> i32 {
        match self {
            Endianness::LittleEndian => buf.get_i32_le(),
            Endianness::BigEndian => buf.get_i32(),
        }
    }

    pub(crate) fn get_u64<B: Buf>(&self, buf: &mut B) -> u64 {
        match self {
            Endianness::LittleEndian => buf.get_u64_le(),
            Endianness::BigEndian => buf.get_u64(),
        }
    }

    pub(crate) fn get_i64<B: Buf>(&self, buf: &mut B) -> i64 {
        match self {
            Endianness::LittleEndian => buf.get_i64_le(),
            Endianness::BigEndian => buf.get_i64(),
        }
    }

    pub(crate) fn get_f64<B: Buf>(&self, buf: &mut B) -> f64 {
        match self {
            Endianness::LittleEndian => buf.get_f64_le(),
            Endianness::BigEndian => buf.get_f64(),
        }
    }

    pub(crate) fn put_u16<B: BufMut>(&self, buf: &mut B, val: u16) {
        match self {
            Endianness::LittleEndian => buf.put_u16_le(val),
            Endianness::BigEndian => buf.put_u16(val),
        }
    }

    pub(crate) fn put_i16<B: BufMut>(&self, buf: &mut B, val: i16) {
        match self {
            Endianness::LittleEndian => buf.put_i16_le(val),
            Endianness::BigEndian => buf.put_i16(val),
        }
    }

    pub(crate) fn put_u32<B: BufMut>(&self, buf: &mut B, val: u32) {
        match self {
            Endianness::LittleEndian => buf.put_u32_le(val),
            Endianness::BigEndian => buf.put_u32(val),
        }
    }

    pub(crate) fn put_i32<B: BufMut>(&self, buf: &mut B, val: i32) {
        match self {
            Endianness::LittleEndian => buf.put_i32_le(val),
            Endianness::BigEndian => buf.put_i32(val),
        }
    }

    pub(crate) fn put_u64<B: BufMut>(&self, buf: &mut B, val: u64) {
        match self {
            Endianness::LittleEndian => buf.put_u64_le(val),
            Endianness::BigEndian => buf.put_u64(val),
        }
    }

    pub(crate) fn put_i64<B: BufMut>(&self, buf: &mut B, val: i64) {
        match self {
            Endianness::LittleEndian => buf.put_i64_le(val),
            Endianness::BigEndian => buf.put_i64(val),
        }
    }

    pub(crate) fn put_f64<B: BufMut>(&self, buf: &mut B, val: f64) {
        match self {
            Endianness::LittleEndian => buf.put_f64_le(val),
            Endianness::BigEndian => buf.put_f64(val),
        }
    }
}
