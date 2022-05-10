//! The `wasmer-compiler` crate provides the necessary abstractions
//! to create a compiler.
//!
//! It provides an universal way of parsing a module via `wasmparser`,
//! while giving the responsibility of compiling specific function
//! WebAssembly bodies to the `Compiler` implementation.

#![deny(missing_docs, trivial_numeric_casts, unused_extern_crates)]
#![warn(unused_import_braces)]
#![cfg_attr(feature = "std", deny(unstable_features))]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "cargo-clippy", allow(clippy::new_without_default))]
#![cfg_attr(
    feature = "cargo-clippy",
    warn(
        clippy::float_arithmetic,
        clippy::mut_mut,
        clippy::nonminimal_bool,
        clippy::map_unwrap_or,
        clippy::print_stdout,
        clippy::unicode_not_nfc,
        clippy::use_self
    )
)]

#[cfg(all(feature = "std", feature = "core"))]
compile_error!(
    "The `std` and `core` features are both enabled, which is an error. Please enable only once."
);

#[cfg(all(not(feature = "std"), not(feature = "core")))]
compile_error!("Both the `std` and `core` features are disabled. Please enable one of them.");

#[cfg(feature = "core")]
extern crate alloc;

mod lib {
    #[cfg(feature = "core")]
    pub mod std {
        pub use alloc::{borrow, boxed, str, string, sync, vec};
        pub use core::fmt;
        pub use hashbrown as collections;
    }

    #[cfg(feature = "std")]
    pub mod std {
        pub use std::{borrow, boxed, collections, fmt, str, string, sync, vec};
    }
}

mod address_map;
#[cfg(feature = "translator")]
mod compiler;
mod function;
mod module;
mod target;
mod trap;
mod unwind;
#[cfg(feature = "translator")]
#[macro_use]
mod translator;
mod sourceloc;

pub use crate::address_map::{FunctionAddressMap, InstructionAddressMap};
#[cfg(feature = "translator")]
pub use crate::compiler::{Compiler, CompilerConfig, Symbol, SymbolRegistry};
pub use crate::function::{
    Compilation, CompiledFunction, CompiledFunctionFrameInfo, CustomSections, Dwarf, FunctionBody,
    Functions,
};
pub use crate::module::CompileModuleInfo;
pub use crate::sourceloc::SourceLoc;
pub use crate::target::{
    Architecture, BinaryFormat, CallingConvention, CpuFeature, Endianness, OperatingSystem,
    PointerWidth, Target, Triple,
};
#[cfg(feature = "translator")]
pub use crate::translator::{
    from_binaryreadererror_wasmerror, translate_module, wptype_to_type, FunctionBinaryReader,
    FunctionBodyData, FunctionMiddleware, MiddlewareBinaryReader, MiddlewareReaderState,
    ModuleEnvironment, ModuleMiddleware, ModuleMiddlewareChain, ModuleTranslationState,
};
pub use crate::trap::TrapInformation;
pub use crate::unwind::CompiledFunctionUnwindInfo;

pub use wasmer_types::{Addend, CodeOffset, Features};

#[cfg(feature = "translator")]
/// wasmparser is exported as a module to slim compiler dependencies
pub use wasmparser;

/// Version number of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
