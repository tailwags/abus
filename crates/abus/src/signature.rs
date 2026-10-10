// SPDX-License-Identifier: Apache-2.0
//! D-Bus type signatures.
//!
//! A signature is never parsed into a tree or a table. The string already is a preorder
//! serialization of the types, so once it is validated the deserializer walks it directly,
//! byte by byte, in lockstep with the data. The only derived fact it ever needs is where an
//! array's element type ends, and [`single_end`] finds that with a short scan when an array
//! is entered.
//!
//! # Validity rules taken from the spec:
//!
//! - The signature is a list of single complete types. Arrays must have element types, and
//!   structs must have both open and close parentheses.
//! - Only type codes, open and close parentheses, and open and close curly brackets are allowed.
//! - Empty structs are not allowed.
//! - A dict entry occurs only as an array element type, has exactly two single complete types,
//!   and its key is a basic type.
//! - The maximum depth of container type nesting is 32 array type codes and 32 open parentheses
//!   (dict entries count as structs, as in libdbus).
//! - The maximum length of a signature is 255.

use std::{borrow::Borrow, fmt, ops::Deref};

const MAX_LEN: usize = 255;
const MAX_DEPTH: u8 = 32;

/// Wire alignment of a value whose type starts with `code`.
pub(crate) const fn alignment(code: u8) -> usize {
    match code {
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b'h' | b's' | b'o' | b'a' => 4,
        b'x' | b't' | b'd' | b'(' | b'{' => 8,
        // y, g, v
        _ => 1,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// Longer than 255 bytes.
    TooLong,
    /// A byte that is not a type code allowed on the bus.
    InvalidCode(u8),
    /// The signature ends inside a type: an `a` without an element, or an unclosed `(`.
    Incomplete,
    /// `()`.
    EmptyStruct,
    /// A dict entry outside an array, with a non-basic key, or without exactly two fields.
    InvalidDictEntry,
    /// More than 32 nested arrays or 32 nested structs.
    TooDeep,
    /// A variant signature that is not exactly one complete type.
    NotSingleType,
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => f.write_str("signature is longer than 255 bytes"),
            Self::InvalidCode(c) => write!(f, "invalid type code {:?} in signature", *c as char),
            Self::Incomplete => f.write_str("signature ends inside a type"),
            Self::EmptyStruct => f.write_str("empty struct in signature"),
            Self::InvalidDictEntry => f.write_str(
                "dict entries must be array elements with a basic key and exactly one value",
            ),
            Self::TooDeep => f.write_str("signature nests more than 32 arrays or 32 structs"),
            Self::NotSingleType => f.write_str("expected exactly one complete type"),
        }
    }
}

impl std::error::Error for SignatureError {}

/// A borrowed, validated D-Bus signature.
///
/// Relates to [`SignatureBuf`] the same way `Path` relates to `PathBuf`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Signature(str);

impl Signature {
    /// The empty signature, which a message without a signature field has.
    pub const EMPTY: &'static Self = unsafe { Self::new_unchecked("") };

    /// Validates `s` as a sequence of zero or more single complete types.
    ///
    /// This is a `const fn`, so a signature literal can be checked at compile time:
    ///
    /// ```
    /// # use abus::Signature;
    /// const SIG: &Signature = match Signature::new("a{sv}") {
    ///     Ok(sig) => sig,
    ///     Err(_) => panic!("invalid signature"),
    /// };
    /// assert_eq!(SIG.as_str(), "a{sv}");
    /// ```
    pub const fn new(s: &str) -> Result<&Self, SignatureError> {
        // `?` is not usable in `const fn`.
        if let Err(e) = validate(s.as_bytes()) {
            return Err(e);
        }

        // SAFETY: validated above.
        Ok(unsafe { Self::new_unchecked(s) })
    }

    /// Returns a reference without validation.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `s` is a valid D-Bus signature.
    pub const unsafe fn new_unchecked(s: &str) -> &Self {
        // SAFETY: repr(transparent) over str.
        unsafe { &*(s as *const str as *const Signature) }
    }

    pub const fn as_str(&self) -> &str {
        &self.0
    }

    pub const fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Whether the signature is exactly one complete type, as a variant's must be.
    pub const fn is_single(&self) -> bool {
        !self.0.is_empty() && single_end(self.as_bytes(), 0) == self.0.len()
    }
}

impl AsRef<str> for Signature {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for Signature {
    fn eq(&self, other: &str) -> bool {
        &self.0 == other
    }
}

impl ToOwned for Signature {
    type Owned = SignatureBuf;

    fn to_owned(&self) -> SignatureBuf {
        // SAFETY: self is a validated signature.
        unsafe { SignatureBuf::new_unchecked(&self.0) }
    }
}

impl AsRef<Signature> for Signature {
    fn as_ref(&self) -> &Signature {
        self
    }
}

impl<'a> TryFrom<&'a str> for &'a Signature {
    type Error = SignatureError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        Signature::new(s)
    }
}

impl PartialEq<SignatureBuf> for Signature {
    fn eq(&self, other: &SignatureBuf) -> bool {
        self.as_str() == other.as_str()
    }
}

/// An owned, validated D-Bus signature.
///
/// Prefer `&Signature` (often `&'static`) and allocate this only when the signature is built at
/// runtime or must outlive its source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SignatureBuf {
    inner: String,
}

impl SignatureBuf {
    /// Validates `s` as a sequence of zero or more single complete types and wraps it.
    pub fn new(s: impl Into<String>) -> Result<Self, SignatureError> {
        let s = s.into();
        validate(s.as_bytes())?;
        Ok(Self { inner: s })
    }

    /// Wraps `s` without validation.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `s` is a valid D-Bus signature.
    pub unsafe fn new_unchecked(s: impl Into<String>) -> Self {
        Self { inner: s.into() }
    }

    pub fn as_signature(&self) -> &Signature {
        self
    }

    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn into_string(self) -> String {
        self.inner
    }
}

impl Deref for SignatureBuf {
    type Target = Signature;

    fn deref(&self) -> &Signature {
        // SAFETY: self.inner is a validated signature.
        unsafe { Signature::new_unchecked(&self.inner) }
    }
}

impl AsRef<Signature> for SignatureBuf {
    fn as_ref(&self) -> &Signature {
        self
    }
}

impl AsRef<str> for SignatureBuf {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl Borrow<Signature> for SignatureBuf {
    fn borrow(&self) -> &Signature {
        self
    }
}

impl From<&Signature> for SignatureBuf {
    fn from(sig: &Signature) -> Self {
        sig.to_owned()
    }
}

impl From<SignatureBuf> for String {
    fn from(sig: SignatureBuf) -> Self {
        sig.inner
    }
}

impl TryFrom<String> for SignatureBuf {
    type Error = SignatureError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl TryFrom<&str> for SignatureBuf {
    type Error = SignatureError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl fmt::Display for SignatureBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl PartialEq<Signature> for SignatureBuf {
    fn eq(&self, other: &Signature) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<&Signature> for SignatureBuf {
    fn eq(&self, other: &&Signature) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<str> for SignatureBuf {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

/// Validates a whole signature: zero or more single complete types.
pub(crate) const fn validate(sig: &[u8]) -> Result<(), SignatureError> {
    if sig.len() > MAX_LEN {
        return Err(SignatureError::TooLong);
    }

    let mut at = 0;

    while at < sig.len() {
        at = match validate_single(sig, at, 0, 0) {
            Ok(end) => end,
            Err(e) => return Err(e),
        };
    }

    Ok(())
}

/// Validates a variant signature: exactly one single complete type.
pub(crate) const fn validate_single_type(sig: &[u8]) -> Result<(), SignatureError> {
    // A variant signature is a `g` value, so its length already fits in a u8.
    if sig.len() > MAX_LEN {
        return Err(SignatureError::TooLong);
    }
    match validate_single(sig, 0, 0, 0) {
        Ok(end) if end == sig.len() => Ok(()),
        Ok(_) => Err(SignatureError::NotSingleType),
        Err(e) => Err(e),
    }
}

/// Validates the single complete type starting at `at` and returns its exclusive end.
/// Recursion is bounded by the depth limits, so it never goes deeper than 64 frames.
///
/// This is a `const fn`, so it avoids `?`, `Option::get` and `Option` comparisons.
const fn validate_single(
    sig: &[u8],
    at: usize,
    arrays: u8,
    structs: u8,
) -> Result<usize, SignatureError> {
    /// `sig.get(at).copied()`, which is not usable in `const fn`.
    #[inline]
    const fn byte_at(sig: &[u8], at: usize) -> Option<u8> {
        if at < sig.len() { Some(sig[at]) } else { None }
    }

    #[inline]
    const fn is_basic(code: u8) -> bool {
        matches!(
            code,
            b'y' | b'b'
                | b'n'
                | b'q'
                | b'i'
                | b'u'
                | b'x'
                | b't'
                | b'd'
                | b'h'
                | b's'
                | b'o'
                | b'g'
        )
    }

    let Some(code) = byte_at(sig, at) else {
        return Err(SignatureError::Incomplete);
    };

    match code {
        _ if is_basic(code) || code == b'v' => Ok(at + 1),
        b'a' => {
            if arrays == MAX_DEPTH {
                return Err(SignatureError::TooDeep);
            }

            if !matches!(byte_at(sig, at + 1), Some(b'{')) {
                return validate_single(sig, at + 1, arrays + 1, structs);
            }

            if structs == MAX_DEPTH {
                return Err(SignatureError::TooDeep);
            }

            let Some(key) = byte_at(sig, at + 2) else {
                return Err(SignatureError::Incomplete);
            };

            if !is_basic(key) || matches!(byte_at(sig, at + 3), Some(b'}')) {
                return Err(SignatureError::InvalidDictEntry);
            }

            let end = match validate_single(sig, at + 3, arrays + 1, structs + 1) {
                Ok(end) => end,
                Err(e) => return Err(e),
            };

            match byte_at(sig, end) {
                Some(b'}') => Ok(end + 1),
                Some(_) => Err(SignatureError::InvalidDictEntry),
                None => Err(SignatureError::Incomplete),
            }
        }
        b'(' => {
            if structs == MAX_DEPTH {
                return Err(SignatureError::TooDeep);
            }

            if matches!(byte_at(sig, at + 1), Some(b')')) {
                return Err(SignatureError::EmptyStruct);
            }

            let mut at = at + 1;

            while !matches!(byte_at(sig, at), Some(b')')) {
                at = match validate_single(sig, at, arrays, structs + 1) {
                    Ok(end) => end,
                    Err(e) => return Err(e),
                };
            }

            Ok(at + 1)
        }
        b'{' => Err(SignatureError::InvalidDictEntry),
        _ => Err(SignatureError::InvalidCode(code)),
    }
}

/// Returns the exclusive end of the single complete type starting at `at`.
///
/// `sig` must be valid: there is no error path, and an invalid signature can panic.
pub(crate) const fn single_end(sig: &[u8], mut at: usize) -> usize {
    let mut depth = 0usize;

    loop {
        match sig[at] {
            // An array prefix is not a complete type on its own.
            b'a' => {
                at += 1;
                continue;
            }
            b'(' | b'{' => depth += 1,
            b')' | b'}' => depth -= 1,
            _ => {}
        }

        at += 1;

        if depth == 0 {
            return at;
        }
    }
}
