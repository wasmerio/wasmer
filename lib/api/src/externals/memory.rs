#[cfg(feature = "js")]
pub use crate::js::externals::memory::Memory;
#[cfg(feature = "sys")]
pub use crate::sys::externals::memory::Memory;
