use std::fmt::{Debug, Display};

#[inline]
fn hex_encode(src: &[u8], dst: &mut [u8]) {
    const HEX_ALPHABET: &[u8; 16] = b"0123456789abcdef";

    for (i, &b) in src.iter().enumerate() {
        dst[i * 2] = HEX_ALPHABET[(b >> 4) as usize];
        dst[i * 2 + 1] = HEX_ALPHABET[(b & 0xf) as usize];
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HexU32 {
    buf: [u8; 20],
    len: u8,
}

impl HexU32 {
    pub fn new(val: u32) -> Self {
        let mut itoa_buf = itoa::Buffer::new();
        let dec = itoa_buf.format(val).as_bytes();

        let mut buf = [0u8; 20];
        hex_encode(dec, &mut buf);

        Self {
            buf,
            len: (dec.len() * 2) as u8,
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: constructed exclusively from HEX_ALPHABET, always valid ASCII
        unsafe { std::str::from_utf8_unchecked(&self.buf[..self.len as usize]) }
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

        let mut inner = [0u8; 32];
        hex_encode(&bytes, &mut inner);

        Ok(Self { inner })
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
