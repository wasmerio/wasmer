//! The `wasmer-compiler` crate provides the necessary abstractions
//! to create a compiler.
//!
//! It provides an universal way of parsing a module via `wasmparser`,
//! while giving the responsibility of compiling specific function
//! WebAssembly bodies to the `Compiler` implementation.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![allow(clippy::new_without_default, clippy::upper_case_acronyms)]
#![warn(
    clippy::float_arithmetic,
    clippy::mut_mut,
    clippy::nonminimal_bool,
    clippy::map_unwrap_or,
    clippy::print_stdout,
    clippy::unicode_not_nfc,
    clippy::use_self
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod engine;
mod traits;

pub mod abi;
pub mod misc;
pub mod object;
pub mod progress;
pub mod serialize;
pub mod types;

pub use crate::engine::*;
pub use crate::traits::*;

mod artifact_builders;

pub use self::artifact_builders::*;

#[cfg(feature = "compiler")]
mod compiler;
#[cfg(feature = "compiler")]
pub use crate::compiler::{
    ArtifactFormatContainer, CompiledFunction, CompiledObjects, Compiler, CompilerConfig, Debugger,
    DeterministicIdComponent, FuncTranslator, FunctionBucket, WASM_LARGE_FUNCTION_THRESHOLD,
    WASM_TRAMPOLINE_ESTIMATED_BODY_SIZE, build_function_buckets, emit_metadata_and_link,
    translate_function_buckets,
};

#[cfg(feature = "compiler")]
pub mod dwarf;
#[cfg(feature = "compiler")]
pub mod elf;
#[cfg(feature = "compiler")]
mod source_map;
#[cfg(feature = "compiler")]
pub use source_map::{SourceLocation, WasmSourceMap};

mod constants;
pub use crate::constants::*;

#[cfg(feature = "translator")]
#[macro_use]
mod translator;
#[cfg(feature = "translator")]
pub use crate::translator::{
    FunctionBinaryReader, FunctionBodyData, FunctionMiddleware, MiddlewareBinaryReader,
    MiddlewareReaderState, ModuleEnvironment, ModuleMiddleware, ModuleMiddlewareChain,
    ModuleTranslationState, from_binaryreadererror_wasmerror, translate_module, wpheaptype_to_type,
    wptype_to_type,
};

pub use wasmer_types::{Addend, CodeOffset, Features};

#[cfg(feature = "translator")]
/// wasmparser is exported as a module to slim compiler dependencies
pub use wasmparser;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
