mod global;
mod thread_local;

#[cfg(any(feature = "sys", feature = "sys-minimal"))]
pub(crate) use global::*;
#[cfg(feature = "js")]
pub(crate) use thread_local::*;
