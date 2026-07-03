use enum_iterator::Sequence;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use std::fmt;

/// The name of a runtime library routine.
///
/// This list is likely to grow over time.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, Hash, Sequence, RkyvSerialize, RkyvDeserialize, Archive,
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

    /// sqrt.f32
    SqrtF32,

    /// sqrt.f64
    SqrtF64,

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

    /// memory.atomic.notify for imported memories
    ImportedMemory32AtomicNotify,

    /// throw
    Throw,

    /// allocate exception object and get an exnref for it
    AllocException,
    /// Get the values buffer pointer out of an exnref
    ReadExnRef,
    /// Given a caught native exception pointer, get the exnref and delete the exception itself
    LibunwindExceptionIntoExnRef,

    /// The personality function
    EHPersonality,
    /// The second stage of the EH personality function
    EHPersonality2,

    /// debug_usize
    DebugUsize,
    /// debug_str
    DebugStr,

    // Soft-float routines emitted by LLVM when targeting platforms without hardware floating-point.
    // Standard toolchains (compiler-rt / libgcc) provide these, but wasmer needs to know their
    // addresses to link JIT-compiled objects.
    // Ordered to match the GCC runtime library documentation (§3.2).
    // https://gcc.gnu.org/onlinedocs/gccint/Soft-float-library-routines.html

    // §3.2.1 Arithmetic
    /// __addsf3
    Addsf3,
    /// __adddf3
    Adddf3,
    /// __subsf3
    Subsf3,
    /// __subdf3
    Subdf3,
    /// __mulsf3
    Mulsf3,
    /// __muldf3
    Muldf3,
    /// __divsf3
    Divsf3,
    /// __divdf3
    Divdf3,
    /// __negsf2
    Negsf2,
    /// __negdf2
    Negdf2,

    // §3.2.2 Conversion
    /// __extendsfdf2
    Extendsfdf2,
    /// __truncdfsf2
    Truncdfsf2,
    /// __fixsfsi
    Fixsfsi,
    /// __fixdfsi
    Fixdfsi,
    /// __fixsfdi
    Fixsfdi,
    /// __fixdfdi
    Fixdfdi,
    /// __fixunssfsi
    Fixunssfsi,
    /// __fixunsdfsi
    Fixunsdfsi,
    /// __fixunssfdi
    Fixunssfdi,
    /// __fixunsdfdi
    Fixunsdfdi,
    /// __floatsisf
    Floatsisf,
    /// __floatsidf
    Floatsidf,
    /// __floatdisf
    Floatdisf,
    /// __floatdidf
    Floatdidf,
    /// __floatunsisf
    Floatunsisf,
    /// __floatunsidf
    Floatunsidf,
    /// __floatundisf
    Floatundisf,
    /// __floatundidf
    Floatundidf,

    // §3.2.3 Comparison
    /// __unordsf2
    Unordsf2,
    /// __unorddf2
    Unorddf2,
    /// __eqsf2
    Eqsf2,
    /// __eqdf2
    Eqdf2,
    /// __nesf2
    Nesf2,
    /// __nedf2
    Nedf2,
    /// __gesf2
    Gesf2,
    /// __gedf2
    Gedf2,
    /// __ltsf2
    Ltsf2,
    /// __ltdf2
    Ltdf2,
    /// __lesf2
    Lesf2,
    /// __ledf2
    Ledf2,
    /// __gtsf2
    Gtsf2,
    /// __gtdf2
    Gtdf2,
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
            Self::SqrtF32 => "wasmer_vm_f32_sqrt",
            Self::SqrtF64 => "wasmer_vm_f64_sqrt",
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
            Self::EHPersonality => "wasmer_eh_personality",
            Self::EHPersonality2 => "wasmer_eh_personality2",
            Self::AllocException => "wasmer_vm_alloc_exception",
            Self::ReadExnRef => "wasmer_vm_read_exnref",
            Self::LibunwindExceptionIntoExnRef => "wasmer_vm_exception_into_exnref",
            Self::DebugUsize => "wasmer_vm_dbg_usize",
            Self::DebugStr => "wasmer_vm_dbg_str",
            // --- Soft-float libcalls ---
            Self::Addsf3 => "__addsf3",
            Self::Adddf3 => "__adddf3",
            Self::Subsf3 => "__subsf3",
            Self::Subdf3 => "__subdf3",
            Self::Mulsf3 => "__mulsf3",
            Self::Muldf3 => "__muldf3",
            Self::Divsf3 => "__divsf3",
            Self::Divdf3 => "__divdf3",
            Self::Negsf2 => "__negsf2",
            Self::Negdf2 => "__negdf2",
            Self::Extendsfdf2 => "__extendsfdf2",
            Self::Truncdfsf2 => "__truncdfsf2",
            Self::Fixsfsi => "__fixsfsi",
            Self::Fixdfsi => "__fixdfsi",
            Self::Fixsfdi => "__fixsfdi",
            Self::Fixdfdi => "__fixdfdi",
            Self::Fixunssfsi => "__fixunssfsi",
            Self::Fixunsdfsi => "__fixunsdfsi",
            Self::Fixunssfdi => "__fixunssfdi",
            Self::Fixunsdfdi => "__fixunsdfdi",
            Self::Floatsisf => "__floatsisf",
            Self::Floatsidf => "__floatsidf",
            Self::Floatdisf => "__floatdisf",
            Self::Floatdidf => "__floatdidf",
            Self::Floatunsisf => "__floatunsisf",
            Self::Floatunsidf => "__floatunsidf",
            Self::Floatundisf => "__floatundisf",
            Self::Floatundidf => "__floatundidf",
            Self::Unordsf2 => "__unordsf2",
            Self::Unorddf2 => "__unorddf2",
            Self::Eqsf2 => "__eqsf2",
            Self::Eqdf2 => "__eqdf2",
            Self::Nesf2 => "__nesf2",
            Self::Nedf2 => "__nedf2",
            Self::Gesf2 => "__gesf2",
            Self::Gedf2 => "__gedf2",
            Self::Ltsf2 => "__ltsf2",
            Self::Ltdf2 => "__ltdf2",
            Self::Lesf2 => "__lesf2",
            Self::Ledf2 => "__ledf2",
            Self::Gtsf2 => "__gtsf2",
            Self::Gtdf2 => "__gtdf2",
        }
    }
}

impl fmt::Display for LibCall {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
