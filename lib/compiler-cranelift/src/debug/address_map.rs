//! Data structures to provide transformation of the source
// addresses of a WebAssembly module into the native code.

use cranelift_codegen::ir;
use wasm_common::entity::PrimaryMap;
use wasm_common::LocalFuncIndex;

/// Value ranges for functions.
pub type ValueLabelsRanges = PrimaryMap<LocalFuncIndex, cranelift_codegen::ValueLabelsRanges>;

/// Stack slots for functions.
pub type StackSlots = PrimaryMap<LocalFuncIndex, ir::StackSlots>;

/// Memory definition offset in the VMContext structure.
#[derive(Debug, Clone)]
pub enum ModuleMemoryOffset {
    /// Not available.
    None,
    /// Offset to the defined memory.
    Defined(u32),
    /// Offset to the imported memory.
    Imported(u32),
}

/// Module `vmctx` related info.
#[derive(Debug, Clone)]
pub struct ModuleVmctxInfo {
    /// The memory definition offset in the VMContext structure.
    pub memory_offset: ModuleMemoryOffset,

    /// The functions stack slots.
    pub stack_slots: StackSlots,
}
