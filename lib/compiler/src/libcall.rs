use loupe::MemoryUsage;
#[cfg(feature = "enable-rkyv")]
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::fmt;

/// The name of a runtime library routine.
///
/// This list is likely to grow over time.
#[cfg_attr(feature = "enable-serde", derive(Deserialize, Serialize))]
#[cfg_attr(
    feature = "enable-rkyv",
    derive(RkyvSerialize, RkyvDeserialize, Archive)
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, MemoryUsage)]
pub enum LibCall {
    /// ceil.f32
    CeilF32,

    /// ceil.f64
    CeilF64,

    /// floor.f32
    FloorF32,

    /// floor.f64
    FloorF64,

    /// nearest.f32
    NearestF32,

    /// nearest.f64
    NearestF64,

    /// trunc.f32
    TruncF32,

    /// trunc.f64
    TruncF64,

    /// memory.size for local functions
    Memory32Size,

    /// memory.size for imported functions
    ImportedMemory32Size,

    /// table.copy
    TableCopy,

    /// table.init
    TableInit,

    /// table.fill
    TableFill,

    /// table.size for local tables
    TableSize,

    /// table.size for imported tables
    ImportedTableSize,

    /// table.get for local tables
    TableGet,

    /// table.get for imported tables
    ImportedTableGet,

    /// table.set for local tables
    TableSet,

    /// table.set for imported tables
    ImportedTableSet,

    /// table.grow for local tables
    TableGrow,

    /// table.grow for imported tables
    ImportedTableGrow,

    /// ref.func
    FuncRef,

    /// elem.drop
    ElemDrop,

    /// memory.copy for local memories
    Memory32Copy,

    /// memory.copy for imported memories
    ImportedMemory32Copy,

    /// memory.fill for local memories
    Memory32Fill,

    /// memory.fill for imported memories
    ImportedMemory32Fill,

    /// memory.init
    Memory32Init,

    /// data.drop
    DataDrop,

    /// A custom trap
    RaiseTrap,

    /// probe for stack overflow. These are emitted for functions which need
    /// when the `enable_probestack` setting is true.
    Probestack,
}

impl fmt::Display for LibCall {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
