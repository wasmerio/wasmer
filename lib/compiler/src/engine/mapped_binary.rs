use std::slice;
use std::{ffi::c_void, ptr};

use itertools::Itertools;
use object::{
    Endianness, Object, ObjectSection, ObjectSegment, ObjectSymbol, ObjectSymbolTable, ReadRef,
    SegmentFlags, SymbolIndex, elf, macho,
};
use wasmer_vm::LibCall;
use wasmer_vm::libcalls::function_pointer;

use crate::engine::unwind::UnwindRegistry;

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
// unconditional because this loader runs on the host while the ELF it processes
// was compiled for the LLVM output target (a runtime value); gating on host
// target_arch would break cross-compilation (e.g. macOS → riscv64).
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

/// Resolves the `LibCall` corresponding to a dynamic relocation symbol emitted
/// into the compiled object file, taking the object's binary format (and, for
/// soft-float routines, its architecture) into account.
fn lookup_libcall(
    name: &str,
    format: object::BinaryFormat,
    architecture: object::Architecture,
) -> Option<LibCall> {
    use object::{Architecture, BinaryFormat};

    let base = match format {
        BinaryFormat::Elf => &LIBCALLS_ELF,
        BinaryFormat::MachO => &LIBCALLS_MACHO,
        _ => return None,
    };
    if let Some(&lc) = base.get(name) {
        return Some(lc);
    }
    // Soft-float libcalls are only emitted by LLVM for RISC-V ELF targets that
    // lack hardware floating-point.  These symbol names never collide with the
    // primary maps, so consulting them whenever the object is RISC-V is safe.
    if format == BinaryFormat::Elf
        && matches!(architecture, Architecture::Riscv32 | Architecture::Riscv64)
        && let Some(&lc) = SOFTFLOAT_LIBCALLS_ELF.get(name)
    {
        return Some(lc);
    }
    None
}

#[derive(Debug)]
struct ImageSegment {
    pub(crate) mem_address: usize,
    pub(crate) mem_size: usize,
    pub(crate) file_address: usize,
    pub(crate) file_size: usize,
    pub(crate) page_size: usize,
    pub(crate) flags: SegmentFlags,
}

impl ImageSegment {
    fn protection(&self) -> Result<i32, String> {
        let (read, write, exec) = match self.flags {
            SegmentFlags::Elf { p_flags } => (
                p_flags & elf::PF_R != 0,
                p_flags & elf::PF_W != 0,
                p_flags & elf::PF_X != 0,
            ),
            SegmentFlags::MachO { initprot, .. } => (
                initprot & macho::VM_PROT_READ != 0,
                initprot & macho::VM_PROT_WRITE != 0,
                initprot & macho::VM_PROT_EXECUTE != 0,
            ),
            _ => return Err(format!("unsupported segment flags: {:?}", self.flags)),
        };

        let mut protection = 0;
        if read {
            protection |= libc::PROT_READ;
        }
        if write {
            protection |= libc::PROT_WRITE;
        }
        if exec {
            protection |= libc::PROT_EXEC;
        }
        Ok(protection)
    }

    fn mem_size_page_aligned(&self) -> usize {
        (self.mem_size + (self.mem_address - self.mem_address_page_aligned()))
            .next_multiple_of(self.page_size)
    }

    fn mem_address_page_aligned(&self) -> usize {
        self.mem_address & !(self.page_size - 1)
    }

    fn file_size_page_aligned(&self) -> usize {
        (self.file_size + (self.file_address - self.file_address_page_aligned()))
            .next_multiple_of(self.page_size)
    }

    fn file_address_page_aligned(&self) -> usize {
        self.file_address & !(self.page_size - 1)
    }
}

// TODO: generate comment
pub(crate) struct MemoryMappedBinary {
    base: *mut c_void,
    size: usize,

    // Unwind registry associated with the binary.
    unwind_registry: Option<UnwindRegistry>,
}

// SAFERY: mmaped base pointer does not escape the type.
unsafe impl Send for MemoryMappedBinary {}
unsafe impl Sync for MemoryMappedBinary {}

impl MemoryMappedBinary {
    pub(crate) fn try_from_file<'a, R: ReadRef<'a>>(
        object_file_fd: i32,
        object_file: &object::File<'a, R>,
        data: R,
    ) -> Result<Self, String> {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };

        let segments = object_file
            .segments()
            .map(|segment| {
                let mem_address = segment.address() as usize;
                let mem_size = segment.size() as usize;
                let (file_address, file_size) = segment.file_range();
                let file_address = file_address as usize;
                let file_size = file_size as usize;
                ImageSegment {
                    mem_address,
                    mem_size,
                    file_address,
                    file_size,
                    page_size,
                    flags: segment.flags(),
                }
            })
            .collect_vec();
        let last_segment = segments
            .last()
            .ok_or("at least one segment is mandatory".to_string())?;
        let total_memory_size =
            last_segment.mem_address_page_aligned() + last_segment.mem_size_page_aligned();

        // Create a contiguous virtual address memory map that will be populated
        // per-partes with the individual protection flags.
        let map = Self::new_mmap(total_memory_size)?;
        let base = map.base();

        // On macOS/Mach-O a file-backed `MAP_FIXED` mapping cannot be created
        // with executable protection (the kernel rejects mapping an unsigned
        // file page as RX), so for Mach-O objects we copy each segment from the
        // file into an anonymous mapping instead of mapping the file directly.
        let is_macho = object_file.format() == object::BinaryFormat::MachO;

        // Mmap individual load segments
        for load_segment in segments {
            // The virtual offset does not need to start at a page boundary.
            if load_segment.file_address % page_size != load_segment.mem_address % page_size {
                return Err(format!(
                    "Load segment file offset 0x{:x} and virtual address 0x{:x} have incompatible page alignment",
                    load_segment.file_address, load_segment.mem_address
                ));
            }

            let protection = load_segment.protection()?;

            let result = if is_macho {
                map.map_copy(
                    load_segment.mem_address_page_aligned(),
                    load_segment.file_size_page_aligned(),
                    protection,
                    object_file_fd,
                    load_segment.file_address_page_aligned(),
                )
            } else {
                map.map(
                    load_segment.mem_address_page_aligned(),
                    load_segment.file_size_page_aligned(),
                    protection,
                    libc::MAP_PRIVATE | libc::MAP_FIXED,
                    object_file_fd,
                    load_segment.file_address_page_aligned(),
                )
            };
            result.map_err(|error| {
                format!(
                    "Cannot map load segment at virtual address 0x{:x}: {error}",
                    load_segment.mem_address_page_aligned()
                )
            })?;

            if load_segment.mem_size_page_aligned() > load_segment.file_size_page_aligned() {
                map.map(
                    load_segment.mem_address_page_aligned() + load_segment.file_size_page_aligned(),
                    load_segment.mem_size_page_aligned() - load_segment.file_size_page_aligned(),
                    protection,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                    -1,
                    0,
                )
                .map_err(|error| format!("Cannot map zero-fill segment tail: {error}"))?;
            }
            if load_segment.mem_size_page_aligned() < load_segment.file_size_page_aligned() {
                return Err("invalid memory segment with larger file representation".to_string());
            }
        }

        // Apply dynamic relocations for the libcalls
        if let Some(dynamic_relocations) = object_file.dynamic_relocations() {
            let dynamic_symbols = object_file.dynamic_symbol_table().unwrap();

            for (offset, relocation) in dynamic_relocations {
                let rel_flags = relocation.flags();
                if matches!(
                    rel_flags,
                    object::RelocationFlags::Elf {
                        r_type: elf::R_X86_64_RELATIVE,
                    }
                ) {
                    unsafe {
                        ptr::write_unaligned(
                            base.add(offset as usize) as *mut usize,
                            (base as usize).wrapping_add(relocation.addend() as usize),
                        );
                    }
                    continue;
                }

                let object::RelocationTarget::Symbol(symbol_index) = relocation.target() else {
                    return Err("unsupported dynamic relocation target".to_string());
                };
                let symbol = dynamic_symbols.symbol_by_index(symbol_index).unwrap();
                let symbol_name = symbol.name().unwrap();
                let Some(libcall) = lookup_libcall(
                    symbol_name,
                    object_file.format(),
                    object_file.architecture(),
                ) else {
                    return Err(format!(
                        "unsupported dynamic relocation symbol {symbol_name}"
                    ));
                };

                let apply_absolute_relocation = || unsafe {
                    ptr::write_unaligned(
                        base.add(offset as usize) as *mut usize,
                        function_pointer(libcall).wrapping_add(relocation.addend() as usize),
                    );
                };
                match (relocation.kind(), rel_flags) {
                    (object::RelocationKind::Absolute, _) => apply_absolute_relocation(),
                    (
                        object::RelocationKind::Unknown,
                        object::RelocationFlags::Elf {
                            r_type: elf::R_X86_64_GLOB_DAT,
                        },
                    ) => apply_absolute_relocation(),
                    (
                        object::RelocationKind::Unknown,
                        object::RelocationFlags::Elf {
                            r_type: elf::R_X86_64_JUMP_SLOT,
                        },
                    ) => apply_absolute_relocation(),
                    kind => return Err(format!("unsupported dynamic relocation kind {kind:?}")),
                }
            }
        }

        // Mach-O objects don't carry ELF-style dynamic relocations; local
        // absolute pointers are slid via the rebase opcode stream and external
        // libcall references are resolved through the indirect symbol table.
        // Rebasing must happen first: it also slides the lazy-pointer slots that
        // `apply_macho_indirect_symbols` then overwrites with libcall addresses.
        if is_macho {
            Self::apply_macho_rebases(base, data)?;
            Self::apply_macho_indirect_symbols(base, data)?;
        }

        Ok(map)
    }

    /// Apply the Mach-O rebase opcodes of a mapped image.
    ///
    /// The linked dylib has a preferred base address of zero but we map it at an
    /// arbitrary `base`, so every absolute pointer the linker embedded (local
    /// `__got` entries such as the C++ typeinfo referenced by exception LSDAs,
    /// lazy pointers, the wasmer function-offset tables, ...) must have the load
    /// slide added. dyld would normally do this from the `LC_DYLD_INFO(_ONLY)`
    /// rebase opcode stream; we interpret that stream here. The slide equals
    /// `base` because the preferred base is zero.
    fn apply_macho_rebases<'a, R: ReadRef<'a>>(base: *mut c_void, data: R) -> Result<(), String> {
        use object::read::macho::MachOFile64;

        let file: MachOFile64<'a, Endianness, R> =
            MachOFile64::parse(data).map_err(|e| format!("cannot parse Mach-O image: {e}"))?;
        let endian = file.endian();

        // Collect segment virtual addresses in load-command order (rebase
        // opcodes reference segments by that index) and locate LC_DYLD_INFO.
        let mut segment_addrs: Vec<u64> = Vec::new();
        let mut dyld_info = None;
        let mut commands = file
            .macho_load_commands()
            .map_err(|e| format!("cannot read Mach-O load commands: {e}"))?;
        while let Some(command) = commands
            .next()
            .map_err(|e| format!("cannot read Mach-O load command: {e}"))?
        {
            if let Some((segment, _)) = command
                .segment_64()
                .map_err(|e| format!("cannot read LC_SEGMENT_64: {e}"))?
            {
                segment_addrs.push(segment.vmaddr.get(endian));
            }
            if let Some(info) = command
                .dyld_info()
                .map_err(|e| format!("cannot read LC_DYLD_INFO: {e}"))?
            {
                dyld_info = Some(info);
            }
        }

        let Some(dyld_info) = dyld_info else {
            return Ok(());
        };
        let rebase_off = dyld_info.rebase_off.get(endian) as u64;
        let rebase_size = dyld_info.rebase_size.get(endian) as u64;
        if rebase_size == 0 {
            return Ok(());
        }
        let opcodes = data
            .read_bytes_at(rebase_off, rebase_size)
            .map_err(|_| "cannot read Mach-O rebase opcodes".to_string())?;

        // Preferred base is zero, so the load slide is the mapping base.
        let slide = base as usize;
        const POINTER_SIZE: u64 = size_of::<usize>() as u64;

        // Adds the slide to the pointer-sized slot at virtual `address`.
        let rebase_pointer = |address: u64| unsafe {
            let slot = base.add(address as usize) as *mut usize;
            let value = ptr::read_unaligned(slot);
            ptr::write_unaligned(slot, value.wrapping_add(slide));
        };

        let read_uleb = |cursor: &mut usize| -> Result<u64, String> {
            let mut result: u64 = 0;
            let mut shift = 0u32;
            loop {
                let byte = *opcodes
                    .get(*cursor)
                    .ok_or_else(|| "truncated Mach-O rebase ULEB".to_string())?;
                *cursor += 1;
                result |= u64::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }
            Ok(result)
        };

        let mut cursor = 0usize;
        let mut address: u64 = 0;
        while cursor < opcodes.len() {
            let byte = opcodes[cursor];
            cursor += 1;
            let opcode = byte & macho::REBASE_OPCODE_MASK;
            let imm = byte & macho::REBASE_IMMEDIATE_MASK;

            match opcode {
                macho::REBASE_OPCODE_DONE => break,
                macho::REBASE_OPCODE_SET_TYPE_IMM => {
                    if imm != macho::REBASE_TYPE_POINTER {
                        return Err(format!(
                            "unsupported Mach-O rebase type {imm}; only pointer rebases are supported"
                        ));
                    }
                }
                macho::REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB => {
                    let offset = read_uleb(&mut cursor)?;
                    let seg_addr = *segment_addrs
                        .get(imm as usize)
                        .ok_or_else(|| format!("Mach-O rebase references unknown segment {imm}"))?;
                    address = seg_addr + offset;
                }
                macho::REBASE_OPCODE_ADD_ADDR_ULEB => {
                    address = address.wrapping_add(read_uleb(&mut cursor)?);
                }
                macho::REBASE_OPCODE_ADD_ADDR_IMM_SCALED => {
                    address = address.wrapping_add(u64::from(imm) * POINTER_SIZE);
                }
                macho::REBASE_OPCODE_DO_REBASE_IMM_TIMES => {
                    for _ in 0..imm {
                        rebase_pointer(address);
                        address = address.wrapping_add(POINTER_SIZE);
                    }
                }
                macho::REBASE_OPCODE_DO_REBASE_ULEB_TIMES => {
                    let count = read_uleb(&mut cursor)?;
                    for _ in 0..count {
                        rebase_pointer(address);
                        address = address.wrapping_add(POINTER_SIZE);
                    }
                }
                macho::REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB => {
                    rebase_pointer(address);
                    address = address
                        .wrapping_add(POINTER_SIZE)
                        .wrapping_add(read_uleb(&mut cursor)?);
                }
                macho::REBASE_OPCODE_DO_REBASE_ULEB_TIMES_SKIPPING_ULEB => {
                    let count = read_uleb(&mut cursor)?;
                    let skip = read_uleb(&mut cursor)?;
                    for _ in 0..count {
                        rebase_pointer(address);
                        address = address.wrapping_add(POINTER_SIZE).wrapping_add(skip);
                    }
                }
                other => {
                    return Err(format!("unknown Mach-O rebase opcode 0x{other:x}"));
                }
            }
        }

        Ok(())
    }

    /// Bind the libcall symbol-pointer slots of a Mach-O image.
    fn apply_macho_indirect_symbols<'a, R: ReadRef<'a>>(
        base: *mut c_void,
        data: R,
    ) -> Result<(), String> {
        use object::read::macho::{MachOFile64, Nlist, Section};

        let file: MachOFile64<'a, Endianness, R> =
            MachOFile64::parse(data).map_err(|e| format!("cannot parse Mach-O image: {e}"))?;
        let endian = file.endian();

        // Locate the LC_DYSYMTAB command; without it there is nothing to bind.
        let mut dysymtab = None;
        let mut commands = file
            .macho_load_commands()
            .map_err(|e| format!("cannot read Mach-O load commands: {e}"))?;
        // TODO: use find instead of the while loop
        while let Some(command) = commands
            .next()
            .map_err(|e| format!("cannot read Mach-O load command: {e}"))?
        {
            if let Some(command) = command
                .dysymtab()
                .map_err(|e| format!("cannot read LC_DYSYMTAB: {e}"))?
            {
                dysymtab = Some(command);
                break;
            }
        }
        let Some(dysymtab) = dysymtab else {
            return Ok(());
        };

        // The indirect symbol table is a flat array of symbol-table indexes
        // shared by every pointer section; each section selects its window into
        // this array via its `reserved1` field.
        let indirect_symbols = dysymtab
            .indirect_symbols(endian, data)
            .map_err(|e| format!("cannot read Mach-O indirect symbols: {e}"))?;
        let symbols = file.macho_symbol_table();
        let architecture = file.architecture();

        for section in file.sections() {
            let raw = section.macho_section();
            match raw.section_type(endian) {
                macho::S_LAZY_SYMBOL_POINTERS | macho::S_NON_LAZY_SYMBOL_POINTERS => {}
                _ => continue,
            }

            // One entry per 8-byte pointer slot, already sliced to this section.
            let entries = raw
                .indirect_symbols(endian, indirect_symbols)
                .map_err(|e| format!("cannot read section indirect symbols: {e}"))?;
            let section_address = section.address() as usize;

            for (slot, entry) in entries.iter().enumerate() {
                let symbol_index = entry.get(endian);
                if symbol_index & (macho::INDIRECT_SYMBOL_LOCAL | macho::INDIRECT_SYMBOL_ABS) != 0 {
                    continue;
                }

                let nlist = symbols
                    .symbol(SymbolIndex(symbol_index as usize))
                    .map_err(|e| format!("invalid Mach-O indirect symbol index: {e}"))?;
                let symbol_name = String::from_utf8_lossy(
                    nlist
                        .name(endian, symbols.strings())
                        .map_err(|e| format!("invalid Mach-O symbol name: {e}"))?,
                );
                if symbol_name == "dyld_stub_binder" {
                    continue;
                }

                // A symbol defined inside this image (e.g. the weak
                // `___wasmer_eh_type_info_N` tag globals). The linker keeps a
                // GOT slot for it because a weak definition can be interposed
                // at runtime, but there is no external symbol to bind: resolve
                // the slot to the symbol's own address inside the mapped image.
                if nlist.is_definition() {
                    unsafe {
                        ptr::write_unaligned(
                            base.add(section_address + slot * size_of::<usize>()) as *mut usize,
                            (base as usize).wrapping_add(nlist.n_value(endian) as usize),
                        );
                    }
                    continue;
                }

                let Some(libcall) =
                    lookup_libcall(&symbol_name, object::BinaryFormat::MachO, architecture)
                else {
                    return Err(format!("unsupported Mach-O indirect symbol {symbol_name}"));
                };

                unsafe {
                    ptr::write_unaligned(
                        base.add(section_address + slot * size_of::<usize>()) as *mut usize,
                        function_pointer(libcall),
                    );
                }
            }
        }

        Ok(())
    }

    fn new_mmap(size: usize) -> Result<Self, String> {
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err("Cannot create a memory map for built Artifact".to_string());
        }

        Ok(Self {
            base,
            size,
            unwind_registry: Some(UnwindRegistry::new()),
        })
    }

    pub(crate) fn base(&self) -> *mut c_void {
        self.base
    }

    /// Returns the mapped memory as a byte slice tied to the lifetime of this map.
    ///
    /// # Safety
    ///
    /// The entire mapped range must be readable for the returned slice's lifetime.
    #[allow(dead_code)]
    unsafe fn as_slice(&self) -> &[u8] {
        if self.base.is_null() || self.size == 0 {
            return &[];
        }

        unsafe { slice::from_raw_parts(self.base.cast::<u8>(), self.size) }
    }

    pub(crate) fn publish_eh_frame_section(
        &mut self,
        address: u64,
        size: u64,
    ) -> Result<(), String> {
        let eh_frame = unsafe {
            slice::from_raw_parts(self.base.cast::<u8>().add(address as usize), size as usize)
        };
        self.unwind_registry
            .as_mut()
            .expect("unwind registry should remain alive until MemoryMap::drop")
            .publish_eh_frame(Some(eh_frame))
    }

    /// Registers the mapped image's linker-produced `__unwind_info` section
    /// (`__TEXT,__unwind_info`) with the unwind registry.
    ///
    /// `address`/`size` are the section's virtual address and size as reported
    /// by the object file; the section is mapped at `base + address`. The Mach-O
    /// header sits at `base`, so it doubles as the `dso_base` every offset in
    /// `__unwind_info` is relative to, and the whole mapped image is the range
    /// the section covers.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub(crate) fn publish_unwind_info_section(
        &mut self,
        address: u64,
        size: u64,
    ) -> Result<(), String> {
        let dso_base = self.base as usize;
        let section_ptr = unsafe { self.base.cast::<u8>().add(address as usize) };
        let covered = dso_base..dso_base + self.size;
        unsafe {
            self.unwind_registry
                .as_mut()
                .expect("unwind registry should remain alive until MemoryMap::drop")
                .publish_unwind_info(dso_base, section_ptr, size as usize, covered)
        }
    }

    fn map(
        &self,
        offset: usize,
        size: usize,
        protection: i32,
        flags: i32,
        fd: i32,
        file_offset: usize,
    ) -> Result<(), String> {
        if offset + size > self.size {
            return Err("Segment will overwrite allocated range".to_string());
        }
        let result = unsafe {
            libc::mmap(
                self.base.add(offset),
                size,
                protection,
                flags,
                fd,
                file_offset as libc::off_t,
            )
        };
        if result == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }

    /// Populate a segment by copying its contents from the file rather than
    /// mapping the file directly.
    ///
    /// TODO: use only for executable segment
    fn map_copy(
        &self,
        offset: usize,
        size: usize,
        protection: i32,
        fd: i32,
        file_offset: usize,
    ) -> Result<(), String> {
        if offset + size > self.size {
            return Err("Segment will overwrite allocated range".to_string());
        }

        // Make the already-reserved anonymous range writable while copying.
        let parent = unsafe { self.base.add(offset) };
        if unsafe { libc::mprotect(parent, size, libc::PROT_READ | libc::PROT_WRITE) } != 0 {
            return Err(format!(
                "Cannot make copied segment writable: {}",
                std::io::Error::last_os_error()
            ));
        }

        // Read the segment contents from the file into a heap buffer. `pread`
        // may return fewer bytes than requested when the (page-aligned) range
        // runs past EOF; the anonymous mapping is already zero-filled, so the
        // tail stays zeroed just as a file-backed mmap would leave it.
        let mut buffer = vec![0u8; size];
        let mut read_total = 0usize;
        while read_total < size {
            let result = unsafe {
                libc::pread(
                    fd,
                    buffer.as_mut_ptr().add(read_total).cast::<c_void>(),
                    size - read_total,
                    (file_offset + read_total) as libc::off_t,
                )
            };
            if result < 0 {
                return Err(format!(
                    "Cannot read segment from object file: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if result == 0 {
                break; // reached EOF; the remaining bytes stay zero-filled
            }
            read_total += result as usize;
        }

        // Copy the heap buffer into the anonymous mapping.
        unsafe {
            ptr::copy_nonoverlapping(buffer.as_ptr(), self.base.add(offset).cast::<u8>(), size);
        }

        // Restrict the range to its real protection now that the bytes are in
        // place (e.g. drop write access and add execute for an RX segment).
        let result = unsafe { libc::mprotect(self.base.add(offset), size, protection) };
        if result != 0 {
            return Err(format!(
                "Cannot set protection on copied segment: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

impl Drop for MemoryMappedBinary {
    fn drop(&mut self) {
        // The registered `.eh_frame` records point into this mmap, so deregister
        // them while the mapping is still live.
        drop(self.unwind_registry.take());

        if !self.base.is_null() && self.size != 0 {
            unsafe {
                libc::munmap(self.base, self.size);
            }
        }
    }
}
