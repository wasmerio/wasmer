#[cfg(feature = "js")]
pub use crate::js::externals::global::Global;
#[cfg(feature = "sys")]
pub use crate::sys::externals::global::Global;
