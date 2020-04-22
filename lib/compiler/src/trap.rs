use crate::CodeOffset;
use serde::{Deserialize, Serialize};
use wasm_common::SourceLoc;
use wasmer_runtime::TrapCode;

/// Information about trap.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct TrapInformation {
    /// The offset of the trapping instruction in native code. It is relative to the beginning of the function.
    pub code_offset: CodeOffset,
    /// Location of trapping instruction in WebAssembly binary module.
    pub source_loc: SourceLoc,
    /// Code of the trap.
    pub trap_code: TrapCode,
}
