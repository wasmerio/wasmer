//! The commands available in the Wasmer binary.
mod cache;
mod compile;
mod config;
#[cfg(all(feature = "object-file", feature = "compiler"))]
mod create_exe;
mod inspect;
mod run;
mod self_update;
mod validate;
#[cfg(feature = "wast")]
mod wast;

#[cfg(all(feature = "object-file", feature = "compiler"))]
pub use create_exe::*;
#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, compile::*, config::*, inspect::*, run::*, self_update::*, validate::*};
