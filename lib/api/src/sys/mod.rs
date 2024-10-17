pub(crate) mod engine;
pub(crate) mod errors;
pub(crate) mod extern_ref;
pub(crate) mod externals;
pub(crate) mod instance;
pub(crate) mod mem_access;
pub(crate) mod module;
pub(crate) mod store;
pub(super) mod tunables;
pub(crate) mod typed_function;
pub(crate) mod vm;

pub use crate::sys::engine::{get_default_compiler_config, NativeEngineExt};
pub use crate::sys::store::NativeStoreExt;
pub use crate::sys::tunables::BaseTunables;
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

pub use wasmer_vm::VMConfig;
