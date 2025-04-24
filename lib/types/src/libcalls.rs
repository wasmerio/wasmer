use enum_iterator::IntoEnumIterator;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::fmt;

/// The name of a runtime library routine.
///
/// This list is likely to grow over time.
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    IntoEnumIterator,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[rkyv(derive(Debug, Hash, PartialEq, Eq), compare(PartialEq))]
#[repr(u16)]
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

    /// memory.atomic.wait32 for local memories
    Memory32AtomicWait32,

    /// memory.atomic.wait32 for imported memories
    ImportedMemory32AtomicWait32,

    /// memory.atomic.wait64 for local memories
    Memory32AtomicWait64,

    /// memory.atomic.wait64 for imported memories
    ImportedMemory32AtomicWait64,

    /// memory.atomic.notify for local memories
    Memory32AtomicNotify,

    /// memory.atomic.botify for imported memories
    ImportedMemory32AtomicNotify,

    /// throw
    Throw,

    /// rethrow
    Rethrow,

    /// alloc_exception
    AllocException,

    /// delete_exception
    DeleteException,

    /// read_exception
    ReadException,

    /// The personality function
    EHPersonality,

    /// debug_usize
    DebugUsize,

    /// __adddf3
    Adddf3,

    /// __addsf3
    Addsf3,

    /// __divdf3
    Divdf3,

    /// __divdi3
    Divdi3,

    /// __divsf3
    Divsf3,

    /// __divsi3
    Divsi3,

    /// __eqdf2
    Eqdf2,

    /// __eqsf2
    Eqsf2,

    /// __extendsfdf2
    Extendsfdf2,

    /// __fixdfdi
    Fixdfdi,

    /// __fixdfsi
    Fixdfsi,

    /// __fixsfdi
    Fixsfdi,

    /// __fixsfsi
    Fixsfsi,

    /// __fixunsdfdi
    Fixunsdfdi,

    /// __fixunsdfsi
    Fixunsdfsi,

    /// __fixunssfdi
    Fixunssfdi,

    /// __fixunssfsi
    Fixunssfsi,

    /// __floatdidf
    Floatdidf,

    /// __floatdisf
    Floatdisf,

    /// __floatsidf
    Floatsidf,

    /// __floatsisf
    Floatsisf,

    /// __floatundidf
    Floatundidf,

    /// __floatundisf
    Floatundisf,

    /// __floatunsidf
    Floatunsidf,

    /// __floatunsisf
    Floatunsisf,

    /// __gedf2
    Gedf2,

    /// __gesf2
    Gesf2,

    /// __gtdf2
    Gtdf2,

    /// __gtsf2
    Gtsf2,

    /// __ledf2
    Ledf2,

    /// __lesf2
    Lesf2,

    /// __ltdf2
    Ltdf2,

    /// __ltsf2
    Ltsf2,

    /// __moddi3
    Moddi3,

    /// __modsi3
    Modsi3,

    /// __muldf3
    Muldf3,

    /// __muldi3
    Muldi3,

    /// __mulsf3
    Mulsf3,

    /// __mulsi3
    Mulsi3,

    /// __nedf2
    Nedf2,

    /// __negdf2
    Negdf2,

    /// __negsf2
    Negsf2,

    /// __nesf2
    Nesf2,

    /// __subdf3
    Subdf3,

    /// __subsf3
    Subsf3,

    /// __truncdfsf2
    Truncdfsf2,

    /// __udivdi3
    Udivdi3,

    /// __udivsi3
    Udivsi3,

    /// __umoddi3
    Umoddi3,

    /// __umodsi3
    Umodsi3,

    /// __unorddf2
    Unorddf2,

    /// __unordsf2
    Unordsf2,

    /// memset
    Memset,

    /// sqrt
    Sqrt,
}

impl LibCall {
    /// Return the function name associated to the libcall.
    pub fn to_function_name(&self) -> &str {
        match self {
            Self::CeilF32 => "wasmer_vm_f32_ceil",
            Self::CeilF64 => "wasmer_vm_f64_ceil",
            Self::FloorF32 => "wasmer_vm_f32_floor",
            Self::FloorF64 => "wasmer_vm_f64_floor",
            Self::NearestF32 => "wasmer_vm_f32_nearest",
            Self::NearestF64 => "wasmer_vm_f64_nearest",
            Self::TruncF32 => "wasmer_vm_f32_trunc",
            Self::TruncF64 => "wasmer_vm_f64_trunc",
            Self::Memory32Size => "wasmer_vm_memory32_size",
            Self::ImportedMemory32Size => "wasmer_vm_imported_memory32_size",
            Self::TableCopy => "wasmer_vm_table_copy",
            Self::TableInit => "wasmer_vm_table_init",
            Self::TableFill => "wasmer_vm_table_fill",
            Self::TableSize => "wasmer_vm_table_size",
            Self::ImportedTableSize => "wasmer_vm_imported_table_size",
            Self::TableGet => "wasmer_vm_table_get",
            Self::ImportedTableGet => "wasmer_vm_imported_table_get",
            Self::TableSet => "wasmer_vm_table_set",
            Self::ImportedTableSet => "wasmer_vm_imported_table_set",
            Self::TableGrow => "wasmer_vm_table_grow",
            Self::ImportedTableGrow => "wasmer_vm_imported_table_grow",
            Self::FuncRef => "wasmer_vm_func_ref",
            Self::ElemDrop => "wasmer_vm_elem_drop",
            Self::Memory32Copy => "wasmer_vm_memory32_copy",
            Self::ImportedMemory32Copy => "wasmer_vm_imported_memory32_copy",
            Self::Memory32Fill => "wasmer_vm_memory32_fill",
            Self::ImportedMemory32Fill => "wasmer_vm_imported_memory32_fill",
            Self::Memory32Init => "wasmer_vm_memory32_init",
            Self::DataDrop => "wasmer_vm_data_drop",
            Self::RaiseTrap => "wasmer_vm_raise_trap",
            // We have to do this because macOS requires a leading `_` and it's not
            // a normal function, it's a static variable, so we have to do it manually.
            #[cfg(target_vendor = "apple")]
            Self::Probestack => "_wasmer_vm_probestack",
            #[cfg(not(target_vendor = "apple"))]
            Self::Probestack => "wasmer_vm_probestack",
            Self::Memory32AtomicWait32 => "wasmer_vm_memory32_atomic_wait32",
            Self::ImportedMemory32AtomicWait32 => "wasmer_vm_imported_memory32_atomic_wait32",
            Self::Memory32AtomicWait64 => "wasmer_vm_memory32_atomic_wait64",
            Self::ImportedMemory32AtomicWait64 => "wasmer_vm_imported_memory32_atomic_wait64",
            Self::Memory32AtomicNotify => "wasmer_vm_memory32_atomic_notify",
            Self::ImportedMemory32AtomicNotify => "wasmer_vm_imported_memory32_atomic_notify",
            Self::Throw => "wasmer_vm_throw",
            Self::Rethrow => "wasmer_vm_rethrow",
            Self::EHPersonality => "wasmer_eh_personality",
            Self::AllocException => "wasmer_vm_alloc_exception",
            Self::DeleteException => "wasmer_vm_delete_exception",
            Self::ReadException => "wasmer_vm_read_exception",
            Self::DebugUsize => "wasmer_vm_dbg_usize",
            Self::Adddf3 => "__adddf3",
            Self::Addsf3 => "__addsf3",
            Self::Divdf3 => "__divdf3",
            Self::Divdi3 => "__divdi3",
            Self::Divsf3 => "__divsf3",
            Self::Divsi3 => "__divsi3",
            Self::Eqdf2 => "__eqdf2",
            Self::Eqsf2 => "__eqsf2",
            Self::Extendsfdf2 => "__extendsfdf2",
            Self::Fixdfdi => "__fixdfdi",
            Self::Fixdfsi => "__fixdfsi",
            Self::Fixsfdi => "__fixsfdi",
            Self::Fixsfsi => "__fixsfsi",
            Self::Fixunsdfdi => "__fixunsdfdi",
            Self::Fixunsdfsi => "__fixunsdfsi",
            Self::Fixunssfdi => "__fixunssfdi",
            Self::Fixunssfsi => "__fixunssfsi",
            Self::Floatdidf => "__floatdidf",
            Self::Floatdisf => "__floatdisf",
            Self::Floatsidf => "__floatsidf",
            Self::Floatsisf => "__floatsisf",
            Self::Floatundidf => "__floatundidf",
            Self::Floatundisf => "__floatundisf",
            Self::Floatunsidf => "__floatunsidf",
            Self::Floatunsisf => "__floatunsisf",
            Self::Gedf2 => "__gedf2",
            Self::Gesf2 => "__gesf2",
            Self::Gtdf2 => "__gtdf2",
            Self::Gtsf2 => "__gtsf2",
            Self::Ledf2 => "__ledf2",
            Self::Lesf2 => "__lesf2",
            Self::Ltdf2 => "__ltdf2",
            Self::Ltsf2 => "__ltsf2",
            Self::Moddi3 => "__moddi3",
            Self::Modsi3 => "__modsi3",
            Self::Muldf3 => "__muldf3",
            Self::Muldi3 => "__muldi3",
            Self::Mulsf3 => "__mulsf3",
            Self::Mulsi3 => "__mulsi3",
            Self::Nedf2 => "__nedf2",
            Self::Negdf2 => "__negdf2",
            Self::Negsf2 => "__negsf2",
            Self::Nesf2 => "__nesf2",
            Self::Subdf3 => "__subdf3",
            Self::Subsf3 => "__subsf3",
            Self::Truncdfsf2 => "__truncdfsf2",
            Self::Udivdi3 => "__udivdi3",
            Self::Udivsi3 => "__udivsi3",
            Self::Umoddi3 => "__umoddi3",
            Self::Umodsi3 => "__umodsi3",
            Self::Unorddf2 => "__unorddf2",
            Self::Unordsf2 => "__unordsf2",
            Self::Memset => "memset",
            Self::Sqrt => "sqrt",
        }
    }
}

impl fmt::Display for LibCall {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
