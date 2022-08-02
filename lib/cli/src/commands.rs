//! The commands available in the Wasmer binary.
#[cfg(target_os = "linux")]
mod binfmt;
mod cache;
#[cfg(feature = "compiler")]
mod compile;
mod config;
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
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
#[cfg(any(feature = "static-artifact-create", feature = "wasmer-artifact-create"))]
pub use create_exe::*;
#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, config::*, inspect::*, run::*, self_update::*, validate::*};
