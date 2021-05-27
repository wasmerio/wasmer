//! Linking for Universal-compiled code.

use std::ptr::write_unaligned;
use wasmer_compiler::{
    JumpTable, JumpTableOffsets, Relocation, RelocationKind, RelocationTarget, Relocations,
    SectionIndex,
};
use wasmer_engine::FunctionExtent;
use wasmer_types::entity::{EntityRef, PrimaryMap};
use wasmer_types::LocalFunctionIndex;
use wasmer_vm::ModuleInfo;
use wasmer_vm::SectionBodyPtr;

fn apply_relocation(
    body: usize,
    r: &Relocation,
    allocated_functions: &PrimaryMap<LocalFunctionIndex, FunctionExtent>,
    jt_offsets: &PrimaryMap<LocalFunctionIndex, JumpTableOffsets>,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBodyPtr>,
) {
    let target_func_address: usize = match r.reloc_target {
        RelocationTarget::LocalFunc(index) => *allocated_functions[index].ptr as usize,
        RelocationTarget::LibCall(libcall) => libcall.function_pointer(),
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
) {
    for (i, section_relocs) in section_relocations.iter() {
        let body = *allocated_sections[i] as usize;
        for r in section_relocs {
            apply_relocation(body, r, allocated_functions, jt_offsets, allocated_sections);
        }
    }
    for (i, function_relocs) in function_relocations.iter() {
        let body = *allocated_functions[i].ptr as usize;
        for r in function_relocs {
            apply_relocation(body, r, allocated_functions, jt_offsets, allocated_sections);
        }
    }
}
