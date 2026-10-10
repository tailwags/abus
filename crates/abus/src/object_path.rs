// SPDX-License-Identifier: Apache-2.0
//! D-Bus object path types.
//!
//! Provides a borrowed [`ObjectPath`] and an owned [`ObjectPathBuf`], mirroring
//! the `Path`/`PathBuf` split. Both enforce the D-Bus spec's validity rules for
//! `OBJECT_PATH`.
//!
//! # Validity rules taken from the spec:
//!
//! - The path may be of any length.
//! - The path must begin with an ASCII '/' (integer 47) character, and must consist of elements separated by slash characters.
//! - Each element must only contain the ASCII characters "[A-Z][a-z][0-9]_"
//! - No element may be the empty string.
//! - Multiple '/' characters cannot occur in sequence.
//! - A trailing '/' character is not allowed unless the path is the root path (a single '/' character).

use std::{borrow::Borrow, fmt, ops::Deref};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectPathError {
    /// Path does not start with `/`.
    MissingLeadingSlash,
    /// An element is empty: caused by `//`, a trailing `/`, or an empty input
    /// after the leading slash.
    EmptyElement,
    /// An element contains a character outside `[A-Za-z0-9_]`.
    InvalidChar,
}

impl fmt::Display for ObjectPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLeadingSlash => f.write_str("object path must begin with '/'"),
            Self::EmptyElement => {
                f.write_str("object path elements must be non-empty (no '//' or trailing '/')")
            }
            Self::InvalidChar => f.write_str("object path elements must match [A-Za-z0-9_]"),
        }
    }
}

impl std::error::Error for ObjectPathError {}

const fn validate(s: &str) -> Result<(), ObjectPathError> {
    let Some((b'/', rest)) = s.as_bytes().split_first() else {
        return Err(ObjectPathError::MissingLeadingSlash);
    };

    // Root path is the only valid single-'/' path.
    if rest.is_empty() {
        return Ok(());
    }

    // Single pass over the bytes after the leading slash (this runs on every received
    // message). An element is empty when a '/' directly follows another '/', or ends the path.
    // Indexed `while` instead of `for`, since iterators are not usable in `const fn`.
    let mut after_slash = true;
    let mut i = 0;

    while i < rest.len() {
        let b = rest[i];

        if b == b'/' {
            if after_slash {
                return Err(ObjectPathError::EmptyElement);
            }

            after_slash = true;
        } else if b.is_ascii_alphanumeric() || b == b'_' {
            after_slash = false;
        } else {
            return Err(ObjectPathError::InvalidChar);
        }

        i += 1;
    }

    if after_slash {
        return Err(ObjectPathError::EmptyElement);
    }

    Ok(())
}

/// A borrowed, validated D-Bus object path.
///
/// Relates to [`ObjectPathBuf`] the same way `Path` relates to `PathBuf`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ObjectPath(str);

impl ObjectPath {
    /// Validates `s` and returns a reference to it as an `ObjectPath`.
    ///
    /// This is a `const fn`, so a path literal can be checked at compile time:
    ///
    /// ```
    /// # use abus::ObjectPath;
    /// const PATH: &ObjectPath = match ObjectPath::new("/org/freedesktop/DBus") {
    ///     Ok(path) => path,
    ///     Err(_) => panic!("invalid object path"),
    /// };
    /// assert_eq!(PATH.as_str(), "/org/freedesktop/DBus");
    /// ```
    pub const fn new(s: &str) -> Result<&Self, ObjectPathError> {
        // `?` is not usable in `const fn`.
        if let Err(e) = validate(s) {
            return Err(e);
        }
        // SAFETY: repr(transparent) over str; validated above.
        Ok(unsafe { Self::new_unchecked(s) })
    }

    /// Returns a reference without validation.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `s` is a valid D-Bus object path.
    pub const unsafe fn new_unchecked(s: &str) -> &Self {
        unsafe { &*(s as *const str as *const ObjectPath) }
    }

    pub const fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if `self` is a namespace prefix of `other`.
    ///
    /// Matches the semantics of the `path_namespace` match rule in the spec:
    /// `other` must equal `self`, or `other` must start with `self` followed
    /// by a `/`. The root path `/` is a prefix of every path.
    ///
    /// ```
    /// # use abus::ObjectPath;
    /// let ns = ObjectPath::new("/com/example/foo").unwrap();
    /// let child = ObjectPath::new("/com/example/foo/bar").unwrap();
    /// let sibling = ObjectPath::new("/com/example/foobar").unwrap();
    ///
    /// assert!(ns.is_namespace_of(child));
    /// assert!(ns.is_namespace_of(ns));
    /// assert!(!ns.is_namespace_of(sibling));
    /// ```
    pub fn is_namespace_of(&self, other: &ObjectPath) -> bool {
        let prefix = self.as_str();
        let child = other.as_str();

        if prefix == "/" {
            return true; // root is a prefix of everything
        }

        if child == prefix {
            return true;
        }

        // child must start with prefix + '/'
        child.starts_with(prefix) && child.as_bytes().get(prefix.len()) == Some(&b'/')
    }
}

impl ToOwned for ObjectPath {
    type Owned = ObjectPathBuf;

    fn to_owned(&self) -> ObjectPathBuf {
        // SAFETY: self is a validated object path.
        unsafe { ObjectPathBuf::new_unchecked(&self.0) }
    }
}

impl AsRef<ObjectPath> for ObjectPath {
    fn as_ref(&self) -> &ObjectPath {
        self
    }
}

impl AsRef<str> for ObjectPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'a> TryFrom<&'a str> for &'a ObjectPath {
    type Error = ObjectPathError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        ObjectPath::new(s)
    }
}

impl fmt::Display for ObjectPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<ObjectPathBuf> for ObjectPath {
    fn eq(&self, other: &ObjectPathBuf) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<str> for ObjectPath {
    fn eq(&self, other: &str) -> bool {
        &self.0 == other
    }
}

/// An owned, validated D-Bus object path.
///
/// Prefer `&ObjectPath` (often `&'static`) and allocate this only when the path is built at
/// runtime or must outlive its source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ObjectPathBuf {
    inner: String,
}

impl ObjectPathBuf {
    /// Validates and wraps `s`.
    pub fn new(s: impl Into<String>) -> Result<Self, ObjectPathError> {
        let s = s.into();
        validate(&s)?;
        Ok(Self { inner: s })
    }

    /// Wraps `s` without validation.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `s` is a valid D-Bus object path.
    pub unsafe fn new_unchecked(s: impl Into<String>) -> Self {
        Self { inner: s.into() }
    }

    pub fn as_object_path(&self) -> &ObjectPath {
        self
    }

    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn into_string(self) -> String {
        self.inner
    }
}

impl Deref for ObjectPathBuf {
    type Target = ObjectPath;

    fn deref(&self) -> &ObjectPath {
        // SAFETY: self.inner is a validated object path.
        unsafe { ObjectPath::new_unchecked(&self.inner) }
    }
}

impl AsRef<ObjectPath> for ObjectPathBuf {
    fn as_ref(&self) -> &ObjectPath {
        self
    }
}

impl AsRef<str> for ObjectPathBuf {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl Borrow<ObjectPath> for ObjectPathBuf {
    fn borrow(&self) -> &ObjectPath {
        self
    }
}

impl From<&ObjectPath> for ObjectPathBuf {
    fn from(path: &ObjectPath) -> Self {
        path.to_owned()
    }
}

impl From<ObjectPathBuf> for String {
    fn from(path: ObjectPathBuf) -> Self {
        path.inner
    }
}

impl TryFrom<String> for ObjectPathBuf {
    type Error = ObjectPathError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl TryFrom<&str> for ObjectPathBuf {
    type Error = ObjectPathError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl fmt::Display for ObjectPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl PartialEq<ObjectPath> for ObjectPathBuf {
    fn eq(&self, other: &ObjectPath) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<&ObjectPath> for ObjectPathBuf {
    fn eq(&self, other: &&ObjectPath) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<str> for ObjectPathBuf {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
