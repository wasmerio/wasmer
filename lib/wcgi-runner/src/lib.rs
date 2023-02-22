// Rust 1.64 doesn't understand tool-specific lints
#![warn(unknown_lints)]
// For now, let's ignore the fact that some of our Error variants are really big
#![allow(clippy::result_large_err)]

pub mod annotations;
mod builder;
mod context;
mod errors;
mod module_loader;
mod runner;

pub use crate::{
    builder::Builder,
    errors::{Error, WebcLoadError},
    runner::Runner,
};
