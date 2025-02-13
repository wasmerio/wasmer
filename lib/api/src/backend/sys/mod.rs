//! Data types, functions and traits for the `sys` runtime.

pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod tunables;
pub mod vm;

pub use engine::NativeEngineExt;
pub use entities::*;
pub use tunables::*;

#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, MiddlewareReaderState, ModuleMiddleware,
};

pub use wasmer_compiler::{
    types::target::{Architecture, CpuFeature, OperatingSystem, Target, Triple},
    Artifact, EngineBuilder, Features, Tunables,
};

pub use wasmer_types::MiddlewareError;

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::{Cranelift, CraneliftOptLevel};
#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::{LLVMOptLevel, LLVM};
#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;
