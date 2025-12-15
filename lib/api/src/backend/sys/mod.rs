//! Data types, functions and traits for the `sys` runtime.

#[cfg(feature = "experimental-async")]
pub(crate) mod async_runtime;
pub(crate) mod entities;
pub(crate) mod error;
pub(crate) mod tunables;
pub mod vm;

pub use engine::NativeEngineExt;
pub use entities::*;
pub use tunables::*;

#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    CompilerConfig, FunctionMiddleware, MiddlewareReaderState, ModuleMiddleware, wasmparser,
};

pub use wasmer_compiler::{Artifact, EngineBuilder, Features, Tunables};

pub use wasmer_types::MiddlewareError;
pub use wasmer_types::target::{Architecture, CpuFeature, OperatingSystem, Target, Triple};

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::{Cranelift, CraneliftOptLevel};
#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::{LLVM, LLVMOptLevel};
#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;
