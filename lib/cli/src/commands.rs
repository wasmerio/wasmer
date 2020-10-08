//! The commands available in the Wasmer binary.
mod cache;
mod compile;
mod config;
#[cfg(feature = "object-file")]
mod create_exe;
mod inspect;
mod run;
mod self_update;
mod validate;
#[cfg(feature = "wast")]
mod wast;

#[cfg(feature = "object-file")]
pub use create_exe::*;
#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, compile::*, config::*, inspect::*, run::*, self_update::*, validate::*};
