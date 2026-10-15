//! Generator for the wasix2 ABI bindings.
//!
//! Pipeline: annotated WIT -> [`parse`] -> [`ir::Model`] -> backend ([`rust`], [`rust_host`], [`c`]).

pub mod annot;
pub mod c;
pub mod format;
pub mod ir;
pub mod parse;
pub mod rust;
pub mod rust_host;

use std::path::Path;

use anyhow::Result;

pub use ir::Target;

/// Parses `wit` and returns formatted Rust source; `source` is recorded in the file header.
pub fn generate_rust(wit: &Path, source: &str, target: Target) -> Result<String> {
    let model = parse::parse_path(wit, target)?;
    format::rustfmt(&rust::generate(&model, source)?)
}

/// Parses `wit` and returns the libc2 files, with paths relative to the libc2 root.
pub fn generate_c(wit: &Path, target: Target) -> Result<Vec<c::GeneratedFile>> {
    c::generate(&parse::parse_path(wit, target)?)
}

/// Parses `wit` and returns the formatted host bindings for `wasix-syscalls2`.
pub fn generate_rust_host(wit: &Path, source: &str, target: Target) -> Result<String> {
    let model = parse::parse_path(wit, target)?;
    format::rustfmt(&rust_host::generate(&model, source)?)
}
