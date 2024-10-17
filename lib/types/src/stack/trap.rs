//! Types for traps.
use crate::CodeOffset;
use crate::TrapCode;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

/// Information about trap.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive, Clone, Copy, Debug, PartialEq, Eq)]
#[rkyv(derive(Debug), compare(PartialEq))]
pub struct TrapInformation {
    /// The offset of the trapping instruction in native code. It is relative to the beginning of the function.
    pub code_offset: CodeOffset,
    /// Code of the trap.
    pub trap_code: TrapCode,
}
