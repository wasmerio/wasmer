//! Data types, functions and traits for the `sys` runtime.

pub(crate) mod entities;
pub use entities::*;
pub(crate) mod error;
pub(crate) mod vm;

#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, MiddlewareReaderState, ModuleMiddleware,
};

pub use wasmer_compiler::{Artifact, EngineBuilder, Features, Tunables};

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::{Cranelift, CraneliftOptLevel};
#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::{LLVMOptLevel, LLVM};
#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;
