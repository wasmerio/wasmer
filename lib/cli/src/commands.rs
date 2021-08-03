//! The commands available in the Wasmer binary.
#[cfg(target_os = "linux")]
mod binfmt;
mod cache;
#[cfg(feature = "compiler")]
mod compile;
mod config;
#[cfg(all(feature = "staticlib", feature = "compiler"))]
mod create_exe;
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
#[cfg(all(feature = "staticlib", feature = "compiler"))]
pub use create_exe::*;
#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, config::*, inspect::*, run::*, self_update::*, validate::*};
