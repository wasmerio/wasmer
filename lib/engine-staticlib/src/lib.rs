//! Staticlib engine for Wasmer compilers.
//!
//! Given a compiler (such as `CraneliftCompiler` or `LLVMCompiler`)
//! it generates a static object file (`.o` file) and metadata which
//! can be used to access it from other programming languages static.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default))]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::option_map_unwrap_or,
        clippy::option_map_unwrap_or_else,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]

mod artifact;
mod builder;
mod engine;
mod serialize;

pub use crate::artifact::StaticlibArtifact;
pub use crate::builder::Staticlib;
pub use crate::engine::StaticlibEngine;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
