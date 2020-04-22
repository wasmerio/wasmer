//! The commands available in the Wasmer binary.
mod cache;
mod run;
mod selfupdate;
mod validate;

pub use run::*;
pub use {cache::*, selfupdate::*, validate::*};
