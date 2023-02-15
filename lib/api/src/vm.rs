//! The `vm` module re-exports wasmer-vm types.

#[cfg(feature = "js")]
pub(crate) use crate::js::vm::{
    VMExtern, VMExternFunction, VMExternGlobal, VMExternMemory, VMExternTable, VMFunction,
    VMFunctionEnvironment, VMGlobal, VMMemory, VMTable,
};

#[cfg(feature = "sys")]
pub(crate) use crate::sys::vm::{
    VMExtern, VMExternFunction, VMExternGlobal, VMExternMemory, VMExternTable,
    VMFunctionEnvironment,
};

// Needed for tunables customization
#[cfg(feature = "sys")]
pub use wasmer_vm::{
    // An extra one for VMMemory implementors
    LinearMemory,
    VMFunction,
    VMGlobal,
    VMMemory,
    VMMemoryDefinition,
    VMTable,
    VMTableDefinition,
};

// Deprecated exports
pub use wasmer_types::{MemoryError, MemoryStyle, TableStyle};
