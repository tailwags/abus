// SPDX-License-Identifier: Apache-2.0
//! Tracing macros that forward to the `tracing` crate when the `tracing`
//! feature is enabled, and expand to nothing otherwise, so call sites never
//! need to be gated. Use them as `crate::tracing::{info, warn, ...}`.

#[cfg(feature = "tracing")]
#[allow(unused_imports)]
pub(crate) use ::tracing::{debug, error, info, trace, warn};

#[cfg(not(feature = "tracing"))]
macro_rules! noop {
    ($($t:tt)*) => {};
}
#[cfg(not(feature = "tracing"))]
#[allow(unused_imports)]
pub(crate) use {noop as debug, noop as error, noop as info, noop as trace, noop as warn};
