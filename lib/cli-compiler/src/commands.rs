//! The commands available in the Wasmer binary.
mod compile;
mod config;
mod validate;

pub use compile::*;
pub use {config::*, validate::*};
