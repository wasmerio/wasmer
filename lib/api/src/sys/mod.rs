pub(crate) mod engine;
pub(crate) mod extern_ref;
pub(crate) mod externals;
pub(crate) mod instance;
pub(crate) mod module;
mod tunables;
pub(crate) mod typed_function;

pub use crate::sys::tunables::BaseTunables;
pub use target_lexicon::{Architecture, CallingConvention, OperatingSystem, Triple, HOST};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, MiddlewareReaderState, ModuleMiddleware,
};
pub use wasmer_compiler::{Features, FrameInfo, LinkError, RuntimeError, Tunables};

// TODO: should those be moved into wasmer::vm as well?
pub use wasmer_vm::{raise_user_trap, MemoryError};
pub(crate) mod vm {
    //! The `vm` module re-exports wasmer-vm types.
    use wasmer_vm::InternalStoreHandle;
    pub(crate) use wasmer_vm::{
        VMExtern, VMExternRef, VMFuncRef, VMFunction, VMFunctionEnvironment, VMGlobal, VMMemory,
        VMTable,
    };

    pub(crate) type VMExternTable = InternalStoreHandle<VMTable>;
    pub(crate) type VMExternMemory = InternalStoreHandle<VMMemory>;
    pub(crate) type VMExternGlobal = InternalStoreHandle<VMGlobal>;
    pub(crate) type VMExternFunction = InternalStoreHandle<VMFunction>;
}

#[cfg(feature = "wat")]
pub use wat::parse_bytes as wat2wasm;

#[cfg(feature = "singlepass")]
pub use wasmer_compiler_singlepass::Singlepass;

#[cfg(feature = "cranelift")]
pub use wasmer_compiler_cranelift::{Cranelift, CraneliftOptLevel};

#[cfg(feature = "llvm")]
pub use wasmer_compiler_llvm::{LLVMOptLevel, LLVM};

#[cfg(feature = "compiler")]
pub use wasmer_compiler::{Artifact, EngineBuilder};
