mod global;
mod thread_local;

#[cfg(feature = "sys")]
pub use global::*;
#[cfg(feature = "js")]
pub use thread_local::*;
