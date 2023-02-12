#[cfg(feature = "js")]
pub use crate::js::externals::memory_view::MemoryView;
#[cfg(feature = "sys")]
pub use crate::sys::externals::memory_view::MemoryView;
