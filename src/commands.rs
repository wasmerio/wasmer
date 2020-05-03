//! The commands available in the Wasmer binary.
mod cache;
mod compile;
mod run;
mod self_update;
mod validate;
#[cfg(feature = "wast")]
mod wast;

#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, compile::*, run::*, self_update::*, validate::*};
