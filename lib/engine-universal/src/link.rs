//! Linking for Universal-compiled code.

use std::collections::HashMap;
use std::ptr::{read_unaligned, write_unaligned};
use wasmer_compiler::{
    JumpTable, JumpTableOffsets, LibCall, Relocation, RelocationKind, RelocationTarget,
    Relocations, SectionIndex, TrampolinesSection,
};
use wasmer_engine::FunctionExtent;
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::{LocalFunctionIndex, ModuleInfo};
use wasmer_vm::SectionBodyPtr;

/// Add a new trampoline address, given the base adress of the Section. Return the address of the jump
/// The trampoline itself still have to be writen
fn trampolines_add(
    map: &mut HashMap<usize, usize>,
    trampoline: &TrampolinesSection,
    address: usize,
    baseaddress: usize,
) -> usize {
    if let Some(target) = map.get(&address) {
        return *target;
    }
    let ret = map.len();
    if ret == trampoline.slots {
        panic!("No more slot in Trampolines");
    }
    map.insert(address, baseaddress + ret * trampoline.size);
    baseaddress + ret * trampoline.size
}

fn use_trampoline(
    address: usize,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBodyPtr>,
    trampolines: &Option<TrampolinesSection>,
    map: &mut HashMap<usize, usize>,
) -> Option<usize> {
    match trampolines {
        Some(trampolines) => Some(trampolines_add(
            map,
            trampolines,
            address,
            *allocated_sections[trampolines.section_index] as usize,
        )),
        _ => None,
    }
}

fn fill_trampolin_map(
    allocated_sections: &PrimaryMap<SectionIndex, SectionBodyPtr>,
    trampolines: &Option<TrampolinesSection>,
) -> HashMap<usize, usize> {
    let mut map: HashMap<usize, usize> = HashMap::new();
    match trampolines {
        Some(trampolines) => {
            let baseaddress = *allocated_sections[trampolines.section_index] as usize;
            for i in 0..trampolines.size {
                let jmpslot: usize = unsafe {
                    read_unaligned((baseaddress + i * trampolines.size + 8) as *mut usize)
                };
                if jmpslot != 0 {
                    map.insert(jmpslot, baseaddress + i * trampolines.size);
                }
            }
        }
        _ => {}
    };
    map
}

/// Given a LibCall it returns the corresponding function pointer so we can link it properly
fn libcall_function_pointer(libcall: LibCall) -> usize {
    use wasmer_vm::libcalls::*;
    match libcall {
        LibCall::CeilF32 => wasmer_vm_f32_ceil as usize,
        LibCall::CeilF64 => wasmer_vm_f64_ceil as usize,
        LibCall::FloorF32 => wasmer_vm_f32_floor as usize,
        LibCall::FloorF64 => wasmer_vm_f64_floor as usize,
        LibCall::NearestF32 => wasmer_vm_f32_nearest as usize,
        LibCall::NearestF64 => wasmer_vm_f64_nearest as usize,
        LibCall::TruncF32 => wasmer_vm_f32_trunc as usize,
        LibCall::TruncF64 => wasmer_vm_f64_trunc as usize,
        LibCall::Memory32Size => wasmer_vm_memory32_size as usize,
        LibCall::ImportedMemory32Size => wasmer_vm_imported_memory32_size as usize,
        LibCall::TableCopy => wasmer_vm_table_copy as usize,
        LibCall::TableInit => wasmer_vm_table_init as usize,
        LibCall::TableFill => wasmer_vm_table_fill as usize,
        LibCall::TableSize => wasmer_vm_table_size as usize,
        LibCall::ImportedTableSize => wasmer_vm_imported_table_size as usize,
        LibCall::TableGet => wasmer_vm_table_get as usize,
        LibCall::ImportedTableGet => wasmer_vm_imported_table_get as usize,
        LibCall::TableSet => wasmer_vm_table_set as usize,
        LibCall::ImportedTableSet => wasmer_vm_imported_table_set as usize,
        LibCall::TableGrow => wasmer_vm_table_grow as usize,
        LibCall::ImportedTableGrow => wasmer_vm_imported_table_grow as usize,
        LibCall::FuncRef => wasmer_vm_func_ref as usize,
        LibCall::ElemDrop => wasmer_vm_elem_drop as usize,
        LibCall::Memory32Copy => wasmer_vm_memory32_copy as usize,
        LibCall::ImportedMemory32Copy => wasmer_vm_imported_memory32_copy as usize,
        LibCall::Memory32Fill => wasmer_vm_memory32_fill as usize,
        LibCall::ImportedMemory32Fill => wasmer_vm_memory32_fill as usize,
        LibCall::Memory32Init => wasmer_vm_memory32_init as usize,
        LibCall::DataDrop => wasmer_vm_data_drop as usize,
        LibCall::Probestack => wasmer_vm_probestack as usize,
        LibCall::RaiseTrap => wasmer_vm_raise_trap as usize,
    }
}

fn apply_relocation(
    body: usize,
    r: &Relocation,
    allocated_functions: &PrimaryMap<LocalFunctionIndex, FunctionExtent>,
    jt_offsets: &PrimaryMap<LocalFunctionIndex, JumpTableOffsets>,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBodyPtr>,
    trampolines: &Option<TrampolinesSection>,
    trampolines_map: &mut HashMap<usize, usize>,
) {
    let target_func_address: usize = match r.reloc_target {
        RelocationTarget::LocalFunc(index) => *allocated_functions[index].ptr as usize,
        RelocationTarget::LibCall(libcall) => libcall_function_pointer(libcall),
        RelocationTarget::CustomSection(custom_section) => {
            *allocated_sections[custom_section] as usize
        }
        RelocationTarget::JumpTable(func_index, jt) => {
            let offset = *jt_offsets
                .get(func_index)
                .and_then(|ofs| ofs.get(JumpTable::new(jt.index())))
                .expect("func jump table");
            *allocated_functions[func_index].ptr as usize + offset as usize
        }
    };

    match r.kind {
        #[cfg(target_pointer_width = "64")]
        RelocationKind::Abs8 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u64, reloc_delta);
        },
        #[cfg(target_pointer_width = "32")]
        RelocationKind::X86PCRel4 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u32, reloc_delta as _);
        },
        #[cfg(target_pointer_width = "64")]
        RelocationKind::X86PCRel8 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u64, reloc_delta);
        },
        RelocationKind::X86CallPCRel4 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u32, reloc_delta as _);
        },
        RelocationKind::X86PCRelRodata4 => {}
        RelocationKind::Arm64Call => unsafe {
            let (reloc_address, mut reloc_delta) = r.for_address(body, target_func_address as u64);
            if (reloc_delta as i64).abs() >= 0x1000_0000 {
                let new_address = match use_trampoline(
                    target_func_address,
                    allocated_sections,
                    trampolines,
                    trampolines_map,
                ) {
                    Some(new_address) => new_address,
                    _ => panic!(
                        "Relocation to big for {:?} for {:?} with {:x}, current val {:x}",
                        r.kind,
                        r.reloc_target,
                        reloc_delta,
                        read_unaligned(reloc_address as *mut u32)
                    ),
                };
                write_unaligned((new_address + 8) as *mut u64, target_func_address as u64); // write the jump address
                let (_, new_delta) = r.for_address(body, new_address as u64);
                reloc_delta = new_delta;
            }
            let reloc_delta = (((reloc_delta / 4) as u32) & 0x3ff_ffff)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw0 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta =
                (((reloc_delta & 0xffff) as u32) << 5) | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw1 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((((reloc_delta >> 16) & 0xffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw2 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((((reloc_delta >> 32) & 0xffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        RelocationKind::Arm64Movw3 => unsafe {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            let reloc_delta = ((((reloc_delta >> 48) & 0xffff) as u32) << 5)
                | read_unaligned(reloc_address as *mut u32);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        kind => panic!(
            "Relocation kind unsupported in the current architecture {}",
            kind
        ),
    }
}

/// Links a module, patching the allocated functions with the
/// required relocations and jump tables.
pub fn link_module(
    _module: &ModuleInfo,
    allocated_functions: &PrimaryMap<LocalFunctionIndex, FunctionExtent>,
    jt_offsets: &PrimaryMap<LocalFunctionIndex, JumpTableOffsets>,
    function_relocations: Relocations,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBodyPtr>,
    section_relocations: &PrimaryMap<SectionIndex, Vec<Relocation>>,
    trampolines: &Option<TrampolinesSection>,
) {
    let mut trampolines_map = fill_trampolin_map(allocated_sections, trampolines);
    for (i, section_relocs) in section_relocations.iter() {
        let body = *allocated_sections[i] as usize;
        for r in section_relocs {
            apply_relocation(
                body,
                r,
                allocated_functions,
                jt_offsets,
                allocated_sections,
                trampolines,
                &mut trampolines_map,
            );
        }
    }
    for (i, function_relocs) in function_relocations.iter() {
        let body = *allocated_functions[i].ptr as usize;
        for r in function_relocs {
            apply_relocation(
                body,
                r,
                allocated_functions,
                jt_offsets,
                allocated_sections,
                trampolines,
                &mut trampolines_map,
            );
        }
    }
}
