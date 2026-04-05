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
