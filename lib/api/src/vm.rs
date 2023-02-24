//! The `vm` module re-exports wasmer-vm types.

#[cfg(feature = "js")]
pub(crate) use crate::js::vm::{
    VMExtern, VMExternFunction, VMExternGlobal, VMExternMemory, VMExternRef, VMExternTable,
    VMFuncRef, VMFunction, VMFunctionBody, VMFunctionEnvironment, VMGlobal, VMInstance, VMMemory,
    VMSharedMemory, VMTable, VMTrampoline,
};

#[cfg(feature = "sys")]
pub(crate) use crate::sys::vm::{
    VMExtern, VMExternFunction, VMExternGlobal, VMExternMemory, VMExternRef, VMExternTable,
    VMFuncRef, VMFunctionBody, VMFunctionEnvironment, VMInstance, VMTrampoline,
};

// Needed for tunables customization (those are public types now)
#[cfg(feature = "sys")]
pub use wasmer_vm::{
    // An extra one for VMMemory implementors
    LinearMemory,
    VMFunction,
    VMGlobal,
    VMMemory,
    VMMemoryDefinition,
    VMSharedMemory,
    VMTable,
    VMTableDefinition,
};

// Deprecated exports
pub use wasmer_types::{MemoryError, MemoryStyle, TableStyle};
