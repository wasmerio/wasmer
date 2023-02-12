pub(crate) mod engine;
pub(crate) mod extern_ref;
pub(crate) mod externals;
pub(crate) mod instance;
pub(crate) mod module;
mod tunables;
pub(crate) mod typed_function;

pub use crate::sys::externals::{
    Extern, FromToNativeWasmType, Function, Global, HostFunction, Memory, MemoryView, Table,
    WasmTypeList,
};

pub use crate::sys::tunables::BaseTunables;
pub use target_lexicon::{Architecture, CallingConvention, OperatingSystem, Triple, HOST};
#[cfg(feature = "compiler")]
pub use wasmer_compiler::{
    wasmparser, CompilerConfig, FunctionMiddleware, MiddlewareReaderState, ModuleMiddleware,
};
pub use wasmer_compiler::{Features, FrameInfo, LinkError, RuntimeError, Tunables};

// TODO: should those be moved into wasmer::vm as well?
pub use wasmer_vm::{raise_user_trap, MemoryError};
pub mod vm {
    //! The `vm` module re-exports wasmer-vm types.

    pub use wasmer_vm::{
        MemoryError, MemoryStyle, TableStyle, VMExtern, VMMemory, VMMemoryDefinition,
        VMOwnedMemory, VMSharedMemory, VMTable, VMTableDefinition,
    };
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
