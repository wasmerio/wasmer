//! The commands available in the Wasmer binary.
mod cache;
mod compile;
mod config;
mod inspect;
mod run;
mod self_update;
mod validate;
#[cfg(feature = "wast")]
mod wast;

#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, compile::*, config::*, inspect::*, run::*, self_update::*, validate::*};
