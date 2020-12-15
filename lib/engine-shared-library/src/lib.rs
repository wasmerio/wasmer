//! Shared Library Engine for Wasmer compilers.
//!
//! Given a compiler (such as `CraneliftCompiler` or `LLVMCompiler`)
//! this engine compiles a WebAssembly module into a shared object
//! library (`.so`, `.dylib` or `.dll` depending on the target), saves
//! it temporarily to disk and uses it natively via `dlopen` and
//! `dlsym` (using the `libloading` library).

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

pub use crate::artifact::SharedLibraryArtifact;
pub use crate::builder::SharedLibrary;
pub use crate::engine::SharedLibraryEngine;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
