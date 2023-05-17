mod global;
mod thread_local;

#[cfg(feature = "sys")]
pub(crate) use global::*;
#[cfg(feature = "js")]
pub(crate) use thread_local::*;
