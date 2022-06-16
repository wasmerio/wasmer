//! The commands available in the Wasmer binary.
#[cfg(target_os = "linux")]
mod binfmt;
mod cache;
#[cfg(feature = "compiler")]
mod compile;
mod config;
mod inspect;
mod run;
mod self_update;
mod validate;
#[cfg(feature = "wast")]
mod wast;

#[cfg(target_os = "linux")]
pub use binfmt::*;
#[cfg(feature = "compiler")]
pub use compile::*;
#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, config::*, inspect::*, run::*, self_update::*, validate::*};
