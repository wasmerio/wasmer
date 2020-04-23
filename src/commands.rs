//! The commands available in the Wasmer binary.
mod cache;
mod run;
mod selfupdate;
mod validate;
#[cfg(feature = "wast")]
mod wast;

pub use run::*;
#[cfg(feature = "wast")]
pub use wast::*;
pub use {cache::*, selfupdate::*, validate::*};
