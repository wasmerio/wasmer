mod global;
mod thread_local;

#[cfg(feature = "sys")]
pub(crate) use global::*;
#[cfg(feature = "web")]
pub(crate) use thread_local::*;
