//! Linking for JIT-compiled code.

use std::ptr::write_unaligned;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::LocalFunctionIndex;
use wasmer_compiler::{
    JumpTable, JumpTableOffsets, Relocation, RelocationKind, RelocationTarget, Relocations,
    SectionBody, SectionIndex,
};
use wasmer_runtime::Module;
use wasmer_runtime::VMFunctionBody;

fn apply_relocation(
    body: usize,
    r: &Relocation,
    allocated_functions: &PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]>,
    jt_offsets: &PrimaryMap<LocalFunctionIndex, JumpTableOffsets>,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBody>,
) {
    let target_func_address: usize = match r.reloc_target {
        RelocationTarget::LocalFunc(index) => {
            let fatptr: *const [VMFunctionBody] = allocated_functions[index];
            fatptr as *const VMFunctionBody as usize
        }
        RelocationTarget::LibCall(libcall) => libcall.function_pointer(),
        RelocationTarget::CustomSection(custom_section) => {
            allocated_sections[custom_section].as_ptr() as usize
        }
        RelocationTarget::JumpTable(func_index, jt) => {
            let offset = *jt_offsets
                .get(func_index)
                .and_then(|ofs| ofs.get(JumpTable::new(jt.index())))
                .expect("func jump table");
            let fatptr: *const [VMFunctionBody] = allocated_functions[func_index];
            fatptr as *const VMFunctionBody as usize + offset as usize
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
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        },
        #[cfg(target_pointer_width = "32")]
        RelocationKind::X86CallPCRel4 => {
            let (reloc_address, reloc_delta) = r.for_address(body, target_func_address as u64);
            write_unaligned(reloc_address as *mut u32, reloc_delta);
        }
        RelocationKind::X86PCRelRodata4 => {}
        _ => panic!("Relocation kind unsupported in the current architecture"),
    }
}

/// Links a module, patching the allocated functions with the
/// required relocations and jump tables.
pub fn link_module(
    _module: &Module,
    allocated_functions: &PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]>,
    jt_offsets: &PrimaryMap<LocalFunctionIndex, JumpTableOffsets>,
    relocations: Relocations,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBody>,
) {
    for (i, function_relocs) in relocations.into_iter() {
        let fatptr: *const [VMFunctionBody] = allocated_functions[i];
        let body = fatptr as *const VMFunctionBody as usize;
        for r in function_relocs {
            apply_relocation(body, r, allocated_functions, jt_offsets, allocated_sections);
        }
    }
}
