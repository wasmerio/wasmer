//! Build-time generator. From the single syscall table (annotated WIT in `wit/`) it emits the
//! guest ABI types, the `Syscalls` trait with its request enum and `Layered` adapter, the wasm
//! import shims, and the libc2 C glue.
//!
//! Pipeline: annotated WIT -> [`parse`] -> [`ir::Model`] -> backend ([`abi`], [`syscalls`],
//! [`exec`], [`c`]). [`shape`] turns functions into the `Syscalls` method signatures.

pub mod abi;
pub mod annot;
pub mod c;
pub mod exec;
pub mod format;
pub mod ir;
pub mod parse;
pub mod rust;
pub mod shape;
pub mod syscalls;

use std::path::Path;

use anyhow::Result;

pub use ir::Target;

/// Parses `wit` and returns the formatted `wasixv2-abi` module; `source` is recorded in the file
/// header.
pub fn generate_abi(wit: &Path, source: &str, target: Target) -> Result<String> {
    let model = parse::parse_path(wit, target)?;
    format::rustfmt(&abi::generate(&model, source)?)
}

/// Parses `wit` and returns the formatted `wasixv2-syscalls` module.
pub fn generate_syscalls(wit: &Path, source: &str, target: Target) -> Result<String> {
    let model = parse::parse_path(wit, target)?;
    format::rustfmt(&syscalls::generate(&model, source)?)
}

/// Parses `wit` and returns the formatted `wasixv2-exec` module.
pub fn generate_exec(wit: &Path, source: &str, target: Target) -> Result<String> {
    let model = parse::parse_path(wit, target)?;
    format::rustfmt(&exec::generate(&model, source)?)
}

/// Parses `wit` and returns the libc2 files, with paths relative to the libc2 root.
pub fn generate_c(wit: &Path, target: Target) -> Result<Vec<c::GeneratedFile>> {
    c::generate(&parse::parse_path(wit, target)?)
}
