#[cfg(feature = "tracing")]
pub use ::tracing::{debug, error, info, trace, warn};

#[cfg(not(feature = "tracing"))]
mod disabled {
    #[allow(unused_macros)]
    macro_rules! error { ($($t:tt)*) => {}; }
    #[allow(unused_macros)]
    macro_rules! _warn { ($($t:tt)*) => {}; }
    #[allow(unused_macros)]
    macro_rules! info  { ($($t:tt)*) => {}; }
    #[allow(unused_macros)]
    macro_rules! debug { ($($t:tt)*) => {}; }
    #[allow(unused_macros)]
    macro_rules! trace { ($($t:tt)*) => {}; }
    pub(crate) use {error, info, debug, trace};
    pub(crate) use _warn as warn;
}

#[cfg(not(feature = "tracing"))]
#[allow(unused_imports)]
pub(crate) use disabled::{debug, error, info, trace, warn};
