//! Data structures to provide transformation of the source
// addresses of a WebAssembly module into the native code.

use cranelift_codegen::ir;
use serde::{Deserialize, Serialize};
use wasm_common::entity::PrimaryMap;
use wasm_common::DefinedFuncIndex;
use wasmer_compiler::SourceLoc;

/// Single source location to generated address mapping.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct InstructionAddressMap {
    /// Original source location.
    pub srcloc: SourceLoc,

    /// Generated instructions offset.
    pub code_offset: usize,

    /// Generated instructions length.
    pub code_len: usize,
}

/// Function and its instructions addresses mappings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FunctionAddressMap {
    /// Instructions maps.
    /// The array is sorted by the InstructionAddressMap::code_offset field.
    pub instructions: Vec<InstructionAddressMap>,

    /// Function start source location (normally declaration).
    pub start_srcloc: SourceLoc,

    /// Function end source location.
    pub end_srcloc: SourceLoc,

    /// Generated function body offset if applicable, otherwise 0.
    pub body_offset: usize,

    /// Generated function body length.
    pub body_len: usize,
}

/// Module functions addresses mappings.
pub type ModuleAddressMap = PrimaryMap<DefinedFuncIndex, FunctionAddressMap>;

/// Value ranges for functions.
pub type ValueLabelsRanges = PrimaryMap<DefinedFuncIndex, cranelift_codegen::ValueLabelsRanges>;

/// Stack slots for functions.
pub type StackSlots = PrimaryMap<DefinedFuncIndex, ir::StackSlots>;

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
