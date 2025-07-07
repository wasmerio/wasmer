mod global;
mod thread_local;

#[cfg(any(feature = "sys", feature = "wasi-common"))]
pub(crate) use global::*;
#[cfg(feature = "js")]
pub(crate) use thread_local::*;
