// SPDX-License-Identifier: Apache-2.0
//! Bounds-checked read cursor shared by the header decoder and the body deserializer.

use crate::{Endianness, utils::align_up};

/// Why a [`Cursor`] read failed. The header decoder turns it into an `io::Error`, the body
/// deserializer into a `de::Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorError {
    /// The value runs past the end of the buffer.
    UnexpectedEof,
    /// Alignment padding contains a non-zero byte.
    NonZeroPadding,
    /// A string is not UTF-8, contains a NUL, or is not NUL-terminated.
    InvalidString,
}

pub(crate) type Result<T> = std::result::Result<T, CursorError>;

/// Read cursor over marshalled D-Bus data. Every read is bounds-checked against `buf`, so
/// malformed input produces an error instead of a panic or an over-read.
///
/// Alignment is computed from `pos`, so `buf` must start at an 8-byte boundary of the message
/// (the frame start for the header, the body start for the body).
///
/// Invariant: `pos <= buf.len()`. Since a slice is at most `isize::MAX` bytes long, adding a
/// D-Bus alignment (at most 8) to `pos` cannot overflow.
///
/// The methods are `#[inline(always)]` because LLVM otherwise sometimes leaves them out of
/// line depending on the caller, which measured about 20 ns slower per decoded header.
pub(crate) struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
    endianness: Endianness,
}

impl<'a> Cursor<'a> {
    /// A cursor at offset `pos` of `buf`. `pos` past the end is clamped to the end.
    #[inline(always)]
    pub(crate) fn new(buf: &'a [u8], pos: usize, endianness: Endianness) -> Self {
        Self {
            buf,
            pos: pos.min(buf.len()),
            endianness,
        }
    }

    #[inline(always)]
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.buf.len()
    }

    #[inline(always)]
    pub(crate) fn endianness(&self) -> Endianness {
        self.endianness
    }

    /// Takes the next `n` bytes.
    #[inline(always)]
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(CursorError::UnexpectedEof)?;
        let bytes = self
            .buf
            .get(self.pos..end)
            .ok_or(CursorError::UnexpectedEof)?;
        self.pos = end;
        Ok(bytes)
    }

    /// Reads `N` bytes without aligning first.
    #[inline(always)]
    pub(crate) fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = *self
            .buf
            .get(self.pos..)
            .and_then(<[u8]>::first_chunk::<N>)
            .ok_or(CursorError::UnexpectedEof)?;
        // `first_chunk` succeeded, so `pos + N <= buf.len()`.
        self.pos += N;
        Ok(bytes)
    }

    /// Skips the padding up to the next multiple of `align` (a power of two), which must be
    /// present and all zero.
    #[inline(always)]
    pub(crate) fn align(&mut self, align: usize) -> Result<()> {
        // No overflow: `pos <= buf.len() <= isize::MAX` and `align <= 8`.
        let to = align_up(self.pos, align);
        let padding = self
            .buf
            .get(self.pos..to)
            .ok_or(CursorError::UnexpectedEof)?;
        if padding.iter().any(|&b| b != 0) {
            return Err(CursorError::NonZeroPadding);
        }
        self.pos = to;
        Ok(())
    }

    /// Reads a fixed-size basic value, aligned to its size as every one of them is.
    #[inline(always)]
    pub(crate) fn aligned<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.align(N)?;
        self.fixed()
    }

    #[inline(always)]
    pub(crate) fn u8(&mut self) -> Result<u8> {
        let [b] = self.fixed()?;
        Ok(b)
    }

    /// Reads a 4-aligned `u32` in the cursor's byte order.
    #[inline(always)]
    pub(crate) fn u32(&mut self) -> Result<u32> {
        let bytes = self.aligned()?;
        Ok(self.endianness.u32_from_bytes(bytes))
    }

    /// Reads `len` bytes of UTF-8 without NULs, then the terminating NUL. The text starts at
    /// the `pos` from before the call.
    #[inline(always)]
    pub(crate) fn str_body(&mut self, len: usize) -> Result<&'a str> {
        let text = self.take(len)?;
        if self.u8()? != 0 {
            return Err(CursorError::InvalidString);
        }
        if is_ascii_without_nul(text) {
            // SAFETY: ASCII is valid UTF-8.
            return Ok(unsafe { std::str::from_utf8_unchecked(text) });
        }
        if text.contains(&0) {
            return Err(CursorError::InvalidString);
        }
        std::str::from_utf8(text).map_err(|_| CursorError::InvalidString)
    }

    /// Reads a `u32`-length string (`s` or `o`). The caller validates object paths.
    #[inline(always)]
    pub(crate) fn string(&mut self) -> Result<&'a str> {
        let len = self.u32()?;
        self.str_body(len as usize)
    }

    /// Reads a `u8`-length signature (`g`). The caller validates it.
    #[inline(always)]
    pub(crate) fn signature(&mut self) -> Result<&'a str> {
        let len = self.u8()?;
        self.str_body(len.into())
    }
}

/// True if every byte is in `1..=0x7f`. Names, paths and signatures almost always are, and
/// checking this a word at a time is cheaper than a NUL search followed by UTF-8 validation.
#[inline(always)]
fn is_ascii_without_nul(bytes: &[u8]) -> bool {
    const LO: u64 = u64::from_ne_bytes([0x01; 8]);
    const HI: u64 = u64::from_ne_bytes([0x80; 8]);
    let (words, rest) = bytes.as_chunks::<8>();
    let mut acc = 0;
    for word in words {
        let x = u64::from_ne_bytes(*word);
        // `(x - LO) & !x` has a high bit set in some byte iff `x` has a zero byte, and `x`
        // has one iff it has a non-ASCII byte.
        acc |= (x.wrapping_sub(LO) & !x) | x;
    }
    acc & HI == 0 && rest.iter().all(|&b| b.wrapping_sub(1) < 0x7f)
}
