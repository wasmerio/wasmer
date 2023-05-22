//! The `vm` module re-exports wasmer-vm types.

#[cfg(feature = "web")]
pub(crate) use crate::web::vm::{
    VMExtern, VMExternFunction, VMExternGlobal, VMExternMemory, VMExternRef, VMExternTable,
    VMFuncRef, VMFunctionCallback, VMFunctionEnvironment, VMInstance, VMTrampoline,
};

#[cfg(feature = "jsc")]
pub(crate) use crate::jsc::vm::{
    VMExtern, VMExternFunction, VMExternGlobal, VMExternMemory, VMExternRef, VMExternTable,
    VMFuncRef, VMFunctionCallback, VMFunctionEnvironment, VMInstance, VMTrampoline,
};

#[cfg(feature = "sys")]
pub(crate) use crate::sys::vm::{
    VMExtern, VMExternFunction, VMExternGlobal, VMExternMemory, VMExternRef, VMExternTable,
    VMFuncRef, VMFunctionCallback, VMFunctionEnvironment, VMInstance, VMTrampoline,
};

#[cfg(feature = "web")]
pub use crate::web::vm::{VMFunction, VMGlobal, VMMemory, VMSharedMemory, VMTable};

#[cfg(feature = "sys")]
pub use wasmer_vm::{VMFunction, VMGlobal, VMMemory, VMSharedMemory, VMTable};

#[cfg(feature = "jsc")]
pub use crate::jsc::vm::{VMFunction, VMGlobal, VMMemory, VMSharedMemory, VMTable};

// Needed for tunables customization (those are public types now)
#[cfg(feature = "sys")]
pub use wasmer_vm::{
    // An extra one for VMMemory implementors
    LinearMemory,
    VMMemoryDefinition,
    VMTableDefinition,
};

// Deprecated exports
pub use wasmer_types::{MemoryError, MemoryStyle, TableStyle};
