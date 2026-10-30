//! Data types, functions and traits for the `js` backend.

pub(crate) mod entities;
pub(crate) mod error;
#[cfg(feature = "experimental-async")]
pub(crate) mod jspi;
pub(crate) mod utils;
pub(crate) mod vm;

pub use entities::*;
pub use utils::*;

#[cfg(feature = "experimental-async")]
mod blocking;
#[cfg(feature = "experimental-async")]
pub use blocking::{BlockingExecutor, set_blocking_executor};
#[cfg(feature = "experimental-async")]
pub use jspi::{AsyncMode, is_supported as is_jspi_supported, set_async_mode};
