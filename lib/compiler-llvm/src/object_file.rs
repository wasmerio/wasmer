use target_lexicon::{
    Architecture, BinaryFormat, Riscv32Architecture, Riscv64Architecture, Triple,
};

use std::num::TryFromIntError;

use wasmer_types::CompileError;

use wasmer_vm::libcalls::LibCall;

fn map_tryfromint_err(error: TryFromIntError) -> CompileError {
    CompileError::Codegen(format!("int doesn't fit: {error}"))
}

fn map_object_err(error: object::read::Error) -> CompileError {
    CompileError::Codegen(format!("error parsing object file: {error}"))
}

static LIBCALLS_ELF: phf::Map<&'static str, LibCall> = phf::phf_map! {
    "ceilf" => LibCall::CeilF32,
    "ceil" => LibCall::CeilF64,
    "floorf" => LibCall::FloorF32,
    "floor" => LibCall::FloorF64,
    "nearbyintf" => LibCall::NearestF32,
    "nearbyint" => LibCall::NearestF64,
    "sqrtf" => LibCall::SqrtF32,
    "sqrt" => LibCall::SqrtF64,
    "truncf" => LibCall::TruncF32,
    "trunc" => LibCall::TruncF64,
    "__chkstk" => LibCall::Probestack,
    "wasmer_vm_f32_ceil" => LibCall::CeilF32,
    "wasmer_vm_f64_ceil" => LibCall::CeilF64,
    "wasmer_vm_f32_floor" => LibCall::FloorF32,
    "wasmer_vm_f64_floor" => LibCall::FloorF64,
    "wasmer_vm_f32_nearest" => LibCall::NearestF32,
    "wasmer_vm_f64_nearest" => LibCall::NearestF64,
    "wasmer_vm_f32_sqrt" => LibCall::SqrtF32,
    "wasmer_vm_f64_sqrt" => LibCall::SqrtF64,
    "wasmer_vm_f32_trunc" => LibCall::TruncF32,
    "wasmer_vm_f64_trunc" => LibCall::TruncF64,
    "wasmer_vm_memory32_size" => LibCall::Memory32Size,
    "wasmer_vm_imported_memory32_size" => LibCall::ImportedMemory32Size,
    "wasmer_vm_table_copy" => LibCall::TableCopy,
    "wasmer_vm_table_init" => LibCall::TableInit,
    "wasmer_vm_table_fill" => LibCall::TableFill,
    "wasmer_vm_table_size" => LibCall::TableSize,
    "wasmer_vm_imported_table_size" => LibCall::ImportedTableSize,
    "wasmer_vm_table_get" => LibCall::TableGet,
    "wasmer_vm_imported_table_get" => LibCall::ImportedTableGet,
    "wasmer_vm_table_set" => LibCall::TableSet,
    "wasmer_vm_imported_table_set" => LibCall::ImportedTableSet,
    "wasmer_vm_table_grow" => LibCall::TableGrow,
    "wasmer_vm_imported_table_grow" => LibCall::ImportedTableGrow,
    "wasmer_vm_func_ref" => LibCall::FuncRef,
    "wasmer_vm_elem_drop" => LibCall::ElemDrop,
    "wasmer_vm_memory32_copy" => LibCall::Memory32Copy,
    "wasmer_vm_imported_memory32_copy" => LibCall::ImportedMemory32Copy,
    "wasmer_vm_memory32_fill" => LibCall::Memory32Fill,
    "wasmer_vm_imported_memory32_fill" => LibCall::ImportedMemory32Fill,
    "wasmer_vm_memory32_init" => LibCall::Memory32Init,
    "wasmer_vm_data_drop" => LibCall::DataDrop,
    "wasmer_vm_raise_trap" => LibCall::RaiseTrap,
    "wasmer_vm_memory32_atomic_wait32" => LibCall::Memory32AtomicWait32,
    "wasmer_vm_imported_memory32_atomic_wait32" => LibCall::ImportedMemory32AtomicWait32,
    "wasmer_vm_memory32_atomic_wait64" => LibCall::Memory32AtomicWait64,
    "wasmer_vm_imported_memory32_atomic_wait64" => LibCall::ImportedMemory32AtomicWait64,
    "wasmer_vm_memory32_atomic_notify" => LibCall::Memory32AtomicNotify,
    "wasmer_vm_imported_memory32_atomic_notify" => LibCall::ImportedMemory32AtomicNotify,
    "wasmer_vm_throw" => LibCall::Throw,
    "wasmer_vm_alloc_exception" => LibCall::AllocException,
    "wasmer_vm_read_exnref" => LibCall::ReadExnRef,
    "wasmer_vm_exception_into_exnref" => LibCall::LibunwindExceptionIntoExnRef,
    "wasmer_eh_personality" => LibCall::EHPersonality,
    "wasmer_eh_personality2" => LibCall::EHPersonality2,
    "wasmer_vm_dbg_usize" => LibCall::DebugUsize,
    "wasmer_vm_dbg_str" => LibCall::DebugStr,
};

// Soft-float routines that LLVM may emit for RISC-V ELF targets.  The map is
// unconditional because `load_object_file` runs on the host while the ELF it
// processes was compiled for the LLVM output target (a runtime value); gating
// on host target_arch would break cross-compilation (e.g. macOS → riscv64).
static SOFTFLOAT_LIBCALLS_ELF: phf::Map<&'static str, LibCall> = phf::phf_map! {
    // §3.2.1 Arithmetic
    "__addsf3" => LibCall::Addsf3,
    "__adddf3" => LibCall::Adddf3,
    "__subsf3" => LibCall::Subsf3,
    "__subdf3" => LibCall::Subdf3,
    "__mulsf3" => LibCall::Mulsf3,
    "__muldf3" => LibCall::Muldf3,
    "__divsf3" => LibCall::Divsf3,
    "__divdf3" => LibCall::Divdf3,
    "__negsf2" => LibCall::Negsf2,
    "__negdf2" => LibCall::Negdf2,
    // §3.2.2 Conversion
    "__extendsfdf2" => LibCall::Extendsfdf2,
    "__truncdfsf2" => LibCall::Truncdfsf2,
    "__fixsfsi" => LibCall::Fixsfsi,
    "__fixdfsi" => LibCall::Fixdfsi,
    "__fixsfdi" => LibCall::Fixsfdi,
    "__fixdfdi" => LibCall::Fixdfdi,
    "__fixunssfsi" => LibCall::Fixunssfsi,
    "__fixunsdfsi" => LibCall::Fixunsdfsi,
    "__fixunssfdi" => LibCall::Fixunssfdi,
    "__fixunsdfdi" => LibCall::Fixunsdfdi,
    "__floatsisf" => LibCall::Floatsisf,
    "__floatsidf" => LibCall::Floatsidf,
    "__floatdisf" => LibCall::Floatdisf,
    "__floatdidf" => LibCall::Floatdidf,
    "__floatunsisf" => LibCall::Floatunsisf,
    "__floatunsidf" => LibCall::Floatunsidf,
    "__floatundisf" => LibCall::Floatundisf,
    "__floatundidf" => LibCall::Floatundidf,
    // §3.2.3 Comparison
    "__unordsf2" => LibCall::Unordsf2,
    "__unorddf2" => LibCall::Unorddf2,
    "__eqsf2" => LibCall::Eqsf2,
    "__eqdf2" => LibCall::Eqdf2,
    "__nesf2" => LibCall::Nesf2,
    "__nedf2" => LibCall::Nedf2,
    "__gesf2" => LibCall::Gesf2,
    "__gedf2" => LibCall::Gedf2,
    "__ltsf2" => LibCall::Ltsf2,
    "__ltdf2" => LibCall::Ltdf2,
    "__lesf2" => LibCall::Lesf2,
    "__ledf2" => LibCall::Ledf2,
    "__gtsf2" => LibCall::Gtsf2,
    "__gtdf2" => LibCall::Gtdf2,
};

static LIBCALLS_MACHO: phf::Map<&'static str, LibCall> = phf::phf_map! {
    "_ceilf" => LibCall::CeilF32,
    "_ceil" => LibCall::CeilF64,
    "_floorf" => LibCall::FloorF32,
    "_floor" => LibCall::FloorF64,
    "_nearbyintf" => LibCall::NearestF32,
    "_nearbyint" => LibCall::NearestF64,
    "_sqrtf" => LibCall::SqrtF32,
    "_sqrt" => LibCall::SqrtF64,
    "_truncf" => LibCall::TruncF32,
    "_trunc" => LibCall::TruncF64,
    "_wasmer_vm_f32_ceil" => LibCall::CeilF32,
    "_wasmer_vm_f64_ceil" => LibCall::CeilF64,
    "_wasmer_vm_f32_floor" => LibCall::FloorF32,
    "_wasmer_vm_f64_floor" => LibCall::FloorF64,
    "_wasmer_vm_f32_nearest" => LibCall::NearestF32,
    "_wasmer_vm_f64_nearest" => LibCall::NearestF64,
    "_wasmer_vm_f32_sqrt" => LibCall::SqrtF32,
    "_wasmer_vm_f64_sqrt" => LibCall::SqrtF64,
    "_wasmer_vm_f32_trunc" => LibCall::TruncF32,
    "_wasmer_vm_f64_trunc" => LibCall::TruncF64,
    "_wasmer_vm_memory32_size" => LibCall::Memory32Size,
    "_wasmer_vm_imported_memory32_size" => LibCall::ImportedMemory32Size,
    "_wasmer_vm_table_copy" => LibCall::TableCopy,
    "_wasmer_vm_table_init" => LibCall::TableInit,
    "_wasmer_vm_table_fill" => LibCall::TableFill,
    "_wasmer_vm_table_size" => LibCall::TableSize,
    "_wasmer_vm_imported_table_size" => LibCall::ImportedTableSize,
    "_wasmer_vm_table_get" => LibCall::TableGet,
    "_wasmer_vm_imported_table_get" => LibCall::ImportedTableGet,
    "_wasmer_vm_table_set" => LibCall::TableSet,
    "_wasmer_vm_imported_table_set" => LibCall::ImportedTableSet,
    "_wasmer_vm_table_grow" => LibCall::TableGrow,
    "_wasmer_vm_imported_table_grow" => LibCall::ImportedTableGrow,
    "_wasmer_vm_func_ref" => LibCall::FuncRef,
    "_wasmer_vm_elem_drop" => LibCall::ElemDrop,
    "_wasmer_vm_memory32_copy" => LibCall::Memory32Copy,
    "_wasmer_vm_imported_memory32_copy" => LibCall::ImportedMemory32Copy,
    "_wasmer_vm_memory32_fill" => LibCall::Memory32Fill,
    "_wasmer_vm_imported_memory32_fill" => LibCall::ImportedMemory32Fill,
    "_wasmer_vm_memory32_init" => LibCall::Memory32Init,
    "_wasmer_vm_data_drop" => LibCall::DataDrop,
    "_wasmer_vm_raise_trap" => LibCall::RaiseTrap,
    "_wasmer_vm_memory32_atomic_wait32" => LibCall::Memory32AtomicWait32,
    "_wasmer_vm_imported_memory32_atomic_wait32" => LibCall::ImportedMemory32AtomicWait32,
    "_wasmer_vm_memory32_atomic_wait64" => LibCall::Memory32AtomicWait64,
    "_wasmer_vm_imported_memory32_atomic_wait64" => LibCall::ImportedMemory32AtomicWait64,
    "_wasmer_vm_memory32_atomic_notify" => LibCall::Memory32AtomicNotify,
    "_wasmer_vm_imported_memory32_atomic_notify" => LibCall::ImportedMemory32AtomicNotify,

    "_wasmer_vm_throw" => LibCall::Throw,
    "_wasmer_vm_alloc_exception" => LibCall::AllocException,
    "_wasmer_vm_read_exnref" => LibCall::ReadExnRef,
    "_wasmer_vm_exception_into_exnref" => LibCall::LibunwindExceptionIntoExnRef,
    // Note: on macOS+Mach-O the personality function *must* be called like this, otherwise LLVM
    // will generate things differently than "normal", wreaking havoc.
    //
    // todo: find out if it is a bug in LLVM or it is expected.
    "___gxx_personality_v0" => LibCall::EHPersonality,
    "_wasmer_eh_personality2" => LibCall::EHPersonality2,
    "_wasmer_vm_dbg_usize" => LibCall::DebugUsize,
    "_wasmer_vm_dbg_str" => LibCall::DebugStr,
};

/// Returns whether `arch` is a RISC-V variant that lacks hardware floating-point
/// (i.e. does not include the F/D ISA extensions, either explicitly or via the `gc` profile).
fn is_riscv_softfloat(arch: &Architecture) -> bool {
    match arch {
        Architecture::Riscv64(Riscv64Architecture::Riscv64gc | Riscv64Architecture::Riscv64a23)
        | Architecture::Riscv32(
            Riscv32Architecture::Riscv32gc | Riscv32Architecture::Riscv32imafc,
        ) => false,
        Architecture::Riscv64(_) | Architecture::Riscv32(_) => true,
        _ => false,
    }
}

// TODO: use in the Artifact resolution code
fn lookup_libcall(name: &str, fmt: BinaryFormat, triple: &Triple) -> Option<LibCall> {
    let base = match fmt {
        BinaryFormat::Elf => &LIBCALLS_ELF,
        BinaryFormat::Macho => &LIBCALLS_MACHO,
        _ => return None,
    };
    if let Some(&lc) = base.get(name) {
        return Some(lc);
    }
    // Soft-float libcalls are only emitted by LLVM for RISC-V targets without
    // hardware floating-point.  We use the runtime LLVM output triple rather than
    // the host target_arch so that cross-compilation (e.g. macOS → riscv64) works.
    if fmt == BinaryFormat::Elf
        && is_riscv_softfloat(&triple.architecture)
        && let Some(&lc) = SOFTFLOAT_LIBCALLS_ELF.get(name)
    {
        return Some(lc);
    }
    None
}
