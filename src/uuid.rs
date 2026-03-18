use std::fmt::{Debug, Display};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Uuid {
    inner: [u8; 32],
}

impl Uuid {
    pub fn new() -> Result<Uuid, getrandom::Error> {
        const HEX_ALPHABET: &[u8; 16] = b"0123456789abcdef";

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

    pub fn as_str(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.inner) }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.inner
    }

    pub fn as_inner(&self) -> &[u8; 32] {
        &self.inner
    }

    pub fn into_inner(self) -> [u8; 32] {
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
