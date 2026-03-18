use std::fmt::{Debug, Display};

const HEX_ALPHABET: &[u8; 16] = b"0123456789abcdef";

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HexU32([u8; 8]);

impl HexU32 {
    #[inline]
    pub const fn new(val: u32) -> Self {
        let b = val.to_be_bytes();
        Self([
            HEX_ALPHABET[(b[0] >> 4) as usize],
            HEX_ALPHABET[(b[0] & 0xf) as usize],
            HEX_ALPHABET[(b[1] >> 4) as usize],
            HEX_ALPHABET[(b[1] & 0xf) as usize],
            HEX_ALPHABET[(b[2] >> 4) as usize],
            HEX_ALPHABET[(b[2] & 0xf) as usize],
            HEX_ALPHABET[(b[3] >> 4) as usize],
            HEX_ALPHABET[(b[3] & 0xf) as usize],
        ])
    }

    #[inline]
    pub const fn as_str(&self) -> &str {
        // SAFETY: constructed exclusively from HEX_ALPHABET, always valid ASCII
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

impl Display for HexU32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Debug for HexU32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HexU32").field(&self.as_str()).finish()
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Uuid {
    inner: [u8; 32],
}

impl Uuid {
    pub fn new() -> Result<Uuid, getrandom::Error> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes)?;

        let mut uuid = [0u8; 32];

        for (i, byte) in bytes.iter().enumerate() {
            let idx = i * 2;
            uuid[idx] = HEX_ALPHABET[(byte >> 4) as usize];
            uuid[idx + 1] = HEX_ALPHABET[(byte & 0x0F) as usize];
        }

        Ok(Self { inner: uuid })
    }

    #[inline]
    pub const fn as_str(&self) -> &str {
        // SAFETY: constructed exclusively from HEX_ALPHABET, always valid ASCII
        unsafe { str::from_utf8_unchecked(&self.inner) }
    }

    #[inline]
    pub const fn as_slice(&self) -> &[u8] {
        &self.inner
    }

    #[inline]
    pub const fn as_inner(&self) -> &[u8; 32] {
        &self.inner
    }

    #[inline]
    pub const fn into_inner(self) -> [u8; 32] {
        self.inner
    }
}

impl Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Debug for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Uuid").field(&self.as_str()).finish()
    }
}
