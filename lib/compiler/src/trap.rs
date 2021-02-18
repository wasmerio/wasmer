use crate::CodeOffset;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_vm::TrapCode;

/// Information about trap.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrapInformation {
    /// The offset of the trapping instruction in native code. It is relative to the beginning of the function.
    pub code_offset: CodeOffset,
    /// Code of the trap.
    pub trap_code: TrapCode,
}
