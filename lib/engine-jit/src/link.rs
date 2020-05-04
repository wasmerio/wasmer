//! Linking for JIT-compiled code.

use std::ptr::write_unaligned;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::LocalFuncIndex;
use wasmer_compiler::{
    JumpTable, JumpTableOffsets, RelocationKind, RelocationTarget, Relocations, SectionBody,
    SectionIndex,
};
use wasmer_runtime::Module;
use wasmer_runtime::VMFunctionBody;

/// Links a module, patching the allocated functions with the
/// requiredd relocations and jump tables.
pub fn link_module(
    module: &Module,
    allocated_functions: &PrimaryMap<LocalFuncIndex, *mut [VMFunctionBody]>,
    jt_offsets: &PrimaryMap<LocalFuncIndex, JumpTableOffsets>,
    relocations: Relocations,
    allocated_sections: &PrimaryMap<SectionIndex, SectionBody>,
) {
    for (i, function_relocs) in relocations.into_iter() {
        for r in function_relocs {
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

            let fatptr: *const [VMFunctionBody] = allocated_functions[i];
            let body = fatptr as *const VMFunctionBody;
            match r.kind {
                #[cfg(target_pointer_width = "64")]
                RelocationKind::Abs8 => unsafe {
                    let reloc_address = body.add(r.offset as usize) as usize;
                    let reloc_addend = r.addend as isize;
                    let reloc_abs = (target_func_address as u64)
                        .checked_add(reloc_addend as u64)
                        .unwrap();
                    write_unaligned(reloc_address as *mut u64, reloc_abs);
                },
                #[cfg(target_pointer_width = "32")]
                RelocationKind::X86PCRel4 => unsafe {
                    let reloc_address = body.add(r.offset as usize) as usize;
                    let reloc_addend = r.addend as isize;
                    let reloc_delta_u32 = (target_func_address as u32)
                        .wrapping_sub(reloc_address as u32)
                        .checked_add(reloc_addend as u32)
                        .unwrap();
                    write_unaligned(reloc_address as *mut u32, reloc_delta_u32);
                },
                #[cfg(target_pointer_width = "32")]
                RelocationKind::X86CallPCRel4 => {
                    let reloc_address = body.add(r.offset as usize) as usize;
                    let reloc_addend = r.addend as isize;
                    let reloc_delta_u32 = (target_func_address as u32)
                        .wrapping_sub(reloc_address as u32)
                        .wrapping_add(reloc_addend as u32);
                    write_unaligned(reloc_address as *mut u32, reloc_delta_u32);
                }
                RelocationKind::X86PCRelRodata4 => {}
                _ => panic!("Relocation kind unsupported"),
            }
        }
    }
}
