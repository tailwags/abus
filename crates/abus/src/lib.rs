// SPDX-License-Identifier: Apache-2.0
mod connection;
mod cursor;
pub mod de;
mod endianness;
pub(crate) mod message;
mod object_path;
pub mod ser;
mod signature;
pub(crate) mod tracing;
pub(crate) mod utils;

pub use connection::Connection;
pub use endianness::Endianness;
pub use message::*;
pub use object_path::{ObjectPath, ObjectPathBuf, ObjectPathError};
pub use ser::Variant;
pub use signature::{Signature, SignatureBuf, SignatureError};
pub use utils::Uuid;

/// Builds a `&'static` [`Signature`] from a string literal, validated at compile time.
///
/// ```
/// const SIG: &abus::Signature = abus::signature!("a{sv}");
/// assert_eq!(SIG.as_str(), "a{sv}");
/// ```
///
/// An invalid signature is a compile error:
///
/// ```compile_fail
/// let _ = abus::signature!("a{vs}");
/// ```
#[macro_export]
macro_rules! signature {
    ($s:literal) => {
        const {
            match $crate::Signature::new($s) {
                ::core::result::Result::Ok(sig) => sig,
                ::core::result::Result::Err(_) => {
                    ::core::panic!("{}", ::core::concat!("invalid D-Bus signature: ", $s))
                }
            }
        }
    };
}

/// Builds a `&'static` [`ObjectPath`] from a string literal, validated at compile time.
///
/// ```
/// const PATH: &abus::ObjectPath = abus::object_path!("/org/freedesktop/DBus");
/// assert_eq!(PATH.as_str(), "/org/freedesktop/DBus");
/// ```
///
/// An invalid path is a compile error:
///
/// ```compile_fail
/// let _ = abus::object_path!("/org/freedesktop/");
/// ```
#[macro_export]
macro_rules! object_path {
    ($s:literal) => {
        const {
            match $crate::ObjectPath::new($s) {
                ::core::result::Result::Ok(path) => path,
                ::core::result::Result::Err(_) => {
                    ::core::panic!("{}", ::core::concat!("invalid D-Bus object path: ", $s))
                }
            }
        }
    };
}
