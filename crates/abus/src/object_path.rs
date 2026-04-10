// SPDX-License-Identifier: Apache-2.0
//! D-Bus object path types.
//!
//! Provides an owned [`ObjectPath`] and a borrowed [`ObjectPathRef`], mirroring
//! the `String`/`str` split. Both enforce the D-Bus spec's validity rules for
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

fn validate(s: &str) -> Result<(), ObjectPathError> {
    let bytes = s.as_bytes();

    if bytes.is_empty() || bytes[0] != b'/' {
        return Err(ObjectPathError::MissingLeadingSlash);
    }

    // Root path is the only valid single-'/' path.
    if bytes == b"/" {
        return Ok(());
    }

    // Skip the leading slash; split('/') catches trailing slashes and double slashes
    // as empty elements.
    for element in s[1..].split('/') {
        if element.is_empty() {
            return Err(ObjectPathError::EmptyElement);
        }
        if !element
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(ObjectPathError::InvalidChar);
        }
    }

    Ok(())
}

/// An owned, validated D-Bus object path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ObjectPath {
    inner: String,
}

impl ObjectPath {
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

    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn into_string(self) -> String {
        self.inner
    }
}

impl Deref for ObjectPath {
    type Target = ObjectPathRef;

    fn deref(&self) -> &ObjectPathRef {
        // SAFETY: self.inner is a validated object path.
        unsafe { ObjectPathRef::new_unchecked(&self.inner) }
    }
}

impl AsRef<ObjectPathRef> for ObjectPath {
    fn as_ref(&self) -> &ObjectPathRef {
        self
    }
}

impl AsRef<str> for ObjectPath {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl Borrow<ObjectPathRef> for ObjectPath {
    fn borrow(&self) -> &ObjectPathRef {
        self
    }
}

impl TryFrom<String> for ObjectPath {
    type Error = ObjectPathError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl TryFrom<&str> for ObjectPath {
    type Error = ObjectPathError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl fmt::Display for ObjectPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

// Cross-type equality
impl PartialEq<ObjectPathRef> for ObjectPath {
    fn eq(&self, other: &ObjectPathRef) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<str> for ObjectPath {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

/// A borrowed, validated D-Bus object path.
///
/// Relates to [`ObjectPath`] the same way `str` relates to `String`.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ObjectPathRef(str);

impl ObjectPathRef {
    /// Validates `s` and returns a reference to it as an `ObjectPathRef`.
    pub fn new(s: &str) -> Result<&Self, ObjectPathError> {
        validate(s)?;
        // SAFETY: repr(transparent) over str; validated above.
        Ok(unsafe { Self::new_unchecked(s) })
    }

    /// Returns a reference without validation.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `s` is a valid D-Bus object path.
    pub unsafe fn new_unchecked(s: &str) -> &Self {
        unsafe { &*(s as *const str as *const ObjectPathRef) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if `self` is a namespace prefix of `other`.
    ///
    /// Matches the semantics of the `path_namespace` match rule in the spec:
    /// `other` must equal `self`, or `other` must start with `self` followed
    /// by a `/`. The root path `/` is a prefix of every path.
    ///
    /// ```
    /// # use abus::object_path::ObjectPathRef;
    /// let ns = ObjectPathRef::new("/com/example/foo").unwrap();
    /// let child = ObjectPathRef::new("/com/example/foo/bar").unwrap();
    /// let sibling = ObjectPathRef::new("/com/example/foobar").unwrap();
    ///
    /// assert!(ns.is_namespace_of(child));
    /// assert!(ns.is_namespace_of(ns));
    /// assert!(!ns.is_namespace_of(sibling));
    /// ```
    pub fn is_namespace_of(&self, other: &ObjectPathRef) -> bool {
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

impl ToOwned for ObjectPathRef {
    type Owned = ObjectPath;

    fn to_owned(&self) -> ObjectPath {
        // SAFETY: self is a validated object path.
        unsafe { ObjectPath::new_unchecked(&self.0) }
    }
}

impl AsRef<str> for ObjectPathRef {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectPathRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<ObjectPath> for ObjectPathRef {
    fn eq(&self, other: &ObjectPath) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<str> for ObjectPathRef {
    fn eq(&self, other: &str) -> bool {
        &self.0 == other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_path_is_valid() {
        assert!(ObjectPath::new("/").is_ok());
    }

    #[test]
    fn simple_path_is_valid() {
        assert!(ObjectPath::new("/foo").is_ok());
    }

    #[test]
    fn nested_path_is_valid() {
        assert!(ObjectPath::new("/com/example/MusicPlayer1").is_ok());
    }

    #[test]
    fn underscores_are_valid() {
        assert!(ObjectPath::new("/com/example/my_service").is_ok());
    }

    #[test]
    fn digits_in_element_are_valid() {
        assert!(ObjectPath::new("/org/freedesktop/DBus").is_ok());
    }

    #[test]
    fn empty_string_is_invalid() {
        assert_eq!(
            ObjectPath::new(""),
            Err(ObjectPathError::MissingLeadingSlash)
        );
    }

    #[test]
    fn no_leading_slash_is_invalid() {
        assert_eq!(
            ObjectPath::new("foo"),
            Err(ObjectPathError::MissingLeadingSlash)
        );
    }

    #[test]
    fn trailing_slash_is_invalid() {
        assert_eq!(ObjectPath::new("/foo/"), Err(ObjectPathError::EmptyElement));
    }

    #[test]
    fn double_slash_is_invalid() {
        assert_eq!(ObjectPath::new("//foo"), Err(ObjectPathError::EmptyElement));
    }

    #[test]
    fn internal_double_slash_is_invalid() {
        assert_eq!(
            ObjectPath::new("/foo//bar"),
            Err(ObjectPathError::EmptyElement)
        );
    }

    #[test]
    fn hyphen_is_invalid() {
        assert_eq!(
            ObjectPath::new("/foo-bar"),
            Err(ObjectPathError::InvalidChar)
        );
    }

    #[test]
    fn dot_is_invalid() {
        assert_eq!(
            ObjectPath::new("/com.example"),
            Err(ObjectPathError::InvalidChar)
        );
    }

    #[test]
    fn space_is_invalid() {
        assert_eq!(
            ObjectPath::new("/foo bar"),
            Err(ObjectPathError::InvalidChar)
        );
    }

    #[test]
    fn namespace_root_matches_everything() {
        let root = ObjectPathRef::new("/").unwrap();
        let other = ObjectPathRef::new("/com/example").unwrap();
        assert!(root.is_namespace_of(other));
    }

    #[test]
    fn namespace_matches_self() {
        let p = ObjectPathRef::new("/com/example/foo").unwrap();
        assert!(p.is_namespace_of(p));
    }

    #[test]
    fn namespace_matches_child() {
        let ns = ObjectPathRef::new("/com/example/foo").unwrap();
        let child = ObjectPathRef::new("/com/example/foo/bar").unwrap();
        assert!(ns.is_namespace_of(child));
    }

    #[test]
    fn namespace_does_not_match_sibling() {
        let ns = ObjectPathRef::new("/com/example/foo").unwrap();
        let sibling = ObjectPathRef::new("/com/example/foobar").unwrap();
        assert!(!ns.is_namespace_of(sibling));
    }

    #[test]
    fn namespace_does_not_match_parent() {
        let ns = ObjectPathRef::new("/com/example/foo").unwrap();
        let parent = ObjectPathRef::new("/com/example").unwrap();
        assert!(!ns.is_namespace_of(parent));
    }

    #[test]
    fn deref_coercion_works() {
        let owned = ObjectPath::new("/foo").unwrap();
        let borrowed: &ObjectPathRef = &owned;
        assert_eq!(borrowed.as_str(), "/foo");
    }

    #[test]
    fn to_owned_round_trips() {
        let borrowed = ObjectPathRef::new("/foo/bar").unwrap();
        let owned = borrowed.to_owned();
        assert_eq!(owned.as_str(), "/foo/bar");
    }

    #[test]
    fn cross_eq_works() {
        let owned = ObjectPath::new("/foo").unwrap();
        let borrowed = ObjectPathRef::new("/foo").unwrap();
        assert_eq!(owned, *borrowed);
        assert_eq!(*borrowed, owned);
    }
}
