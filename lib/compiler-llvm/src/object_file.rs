use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use wasmer_compiler::{
    CompileError, CompiledFunctionFrameInfo, CustomSection, CustomSectionProtection,
    CustomSections, FunctionAddressMap, FunctionBody, InstructionAddressMap, Relocation,
    RelocationKind, RelocationTarget, SectionBody, SectionIndex, SourceLoc,
};
use wasmer_types::entity::{PrimaryMap, SecondaryMap};
use wasmer_vm::libcalls::LibCall;

use wasmer_types::entity::entity_impl;
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ElfSectionIndex(u32);
entity_impl!(ElfSectionIndex);
impl ElfSectionIndex {
    pub fn is_undef(&self) -> bool {
        self.as_u32() == goblin::elf::section_header::SHN_UNDEF
    }

    pub fn from_usize(value: usize) -> Result<Self, CompileError> {
        match u32::try_from(value) {
            Err(_) => Err(CompileError::Codegen(format!(
                "elf section index {} does not fit in 32 bits",
                value
            ))),
            Ok(value) => Ok(ElfSectionIndex::from_u32(value)),
        }
    }

    pub fn as_usize(&self) -> usize {
        self.as_u32() as usize
    }
}

fn map_goblin_err(error: goblin::error::Error) -> CompileError {
    CompileError::Codegen(format!("error parsing ELF file: {}", error))
}

pub struct CompiledFunction {
    pub compiled_function: wasmer_compiler::CompiledFunction,
    pub custom_sections: CustomSections,
    pub eh_frame_section_indices: Vec<SectionIndex>,
}

pub fn load_object_file<F>(
    contents: &[u8],
    root_section: &str,
    root_section_reloc_target: RelocationTarget,
    mut symbol_name_to_relocation_target: F,
) -> Result<CompiledFunction, CompileError>
where
    F: FnMut(&String) -> Result<Option<RelocationTarget>, CompileError>,
{
    // TODO: use perfect hash function?
    let mut libcalls = HashMap::new();
    libcalls.insert("ceilf".to_string(), LibCall::CeilF32);
    libcalls.insert("ceil".to_string(), LibCall::CeilF64);
    libcalls.insert("floorf".to_string(), LibCall::FloorF32);
    libcalls.insert("floor".to_string(), LibCall::FloorF64);
    libcalls.insert("nearbyintf".to_string(), LibCall::NearestF32);
    libcalls.insert("nearbyint".to_string(), LibCall::NearestF64);
    libcalls.insert("truncf".to_string(), LibCall::TruncF32);
    libcalls.insert("trunc".to_string(), LibCall::TruncF64);
    libcalls.insert("wasmer_vm_f32_ceil".to_string(), LibCall::CeilF32);
    libcalls.insert("wasmer_vm_f64_ceil".to_string(), LibCall::CeilF64);
    libcalls.insert("wasmer_vm_f32_floor".to_string(), LibCall::FloorF32);
    libcalls.insert("wasmer_vm_f64_floor".to_string(), LibCall::FloorF64);
    libcalls.insert("wasmer_vm_f32_nearest".to_string(), LibCall::NearestF32);
    libcalls.insert("wasmer_vm_f64_nearest".to_string(), LibCall::NearestF64);
    libcalls.insert("wasmer_vm_f32_trunc".to_string(), LibCall::TruncF32);
    libcalls.insert("wasmer_vm_f64_trunc".to_string(), LibCall::TruncF64);
    libcalls.insert("wasmer_vm_memory32_size".to_string(), LibCall::Memory32Size);
    libcalls.insert(
        "wasmer_vm_imported_memory32_size".to_string(),
        LibCall::ImportedMemory32Size,
    );
    libcalls.insert("wasmer_vm_table_copy".to_string(), LibCall::TableCopy);
    libcalls.insert("wasmer_vm_table_init".to_string(), LibCall::TableInit);
    libcalls.insert("wasmer_vm_table_fill".to_string(), LibCall::TableFill);
    libcalls.insert("wasmer_vm_table_size".to_string(), LibCall::TableSize);
    libcalls.insert(
        "wasmer_vm_imported_table_size".to_string(),
        LibCall::ImportedTableSize,
    );
    libcalls.insert("wasmer_vm_table_get".to_string(), LibCall::TableGet);
    libcalls.insert(
        "wasmer_vm_imported_table_get".to_string(),
        LibCall::ImportedTableGet,
    );
    libcalls.insert("wasmer_vm_table_set".to_string(), LibCall::TableSet);
    libcalls.insert(
        "wasmer_vm_imported_table_set".to_string(),
        LibCall::ImportedTableSet,
    );
    libcalls.insert("wasmer_vm_table_grow".to_string(), LibCall::TableGrow);
    libcalls.insert(
        "wasmer_vm_imported_table_grow".to_string(),
        LibCall::ImportedTableGrow,
    );
    libcalls.insert("wasmer_vm_func_ref".to_string(), LibCall::FuncRef);
    libcalls.insert("wasmer_vm_elem_drop".to_string(), LibCall::ElemDrop);
    libcalls.insert("wasmer_vm_memory32_copy".to_string(), LibCall::Memory32Copy);
    libcalls.insert(
        "wasmer_vm_imported_memory32_copy".to_string(),
        LibCall::ImportedMemory32Copy,
    );
    libcalls.insert("wasmer_vm_memory32_fill".to_string(), LibCall::Memory32Fill);
    libcalls.insert(
        "wasmer_vm_imported_memory32_fill".to_string(),
        LibCall::ImportedMemory32Fill,
    );
    libcalls.insert("wasmer_vm_memory32_init".to_string(), LibCall::Memory32Init);
    libcalls.insert("wasmer_vm_data_drop".to_string(), LibCall::DataDrop);
    libcalls.insert("wasmer_vm_raise_trap".to_string(), LibCall::RaiseTrap);
    libcalls.insert("wasmer_vm_probestack".to_string(), LibCall::Probestack);

    let elf = goblin::elf::Elf::parse(&contents).map_err(map_goblin_err)?;
    let get_section_name = |section: &goblin::elf::section_header::SectionHeader| {
        if section.sh_name == goblin::elf::section_header::SHN_UNDEF as _ {
            return None;
        }
        elf.strtab.get(section.sh_name)?.ok()
    };

    // Build up a mapping from a section to its relocation sections.
    let reloc_sections = elf.shdr_relocs.iter().fold(
        HashMap::new(),
        |mut map: HashMap<_, Vec<_>>, (section_index, reloc_section)| {
            let target_section = elf.section_headers[*section_index].sh_info as usize;
            let target_section = ElfSectionIndex::from_usize(target_section).unwrap();
            map.entry(target_section).or_default().push(reloc_section);
            map
        },
    );

    let mut visited: HashSet<ElfSectionIndex> = HashSet::new();
    let mut worklist: Vec<ElfSectionIndex> = Vec::new();
    let mut section_targets: HashMap<ElfSectionIndex, RelocationTarget> = HashMap::new();

    let root_section_index = elf
        .section_headers
        .iter()
        .enumerate()
        .filter(|(_, section)| get_section_name(section) == Some(root_section))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if root_section_index.len() != 1 {
        return Err(CompileError::Codegen(format!(
            "found {} sections named {}",
            root_section_index.len(),
            root_section
        )));
    }
    let root_section_index = root_section_index[0];
    let root_section_index = ElfSectionIndex::from_usize(root_section_index)?;

    let mut section_to_custom_section = HashMap::new();

    section_targets.insert(root_section_index, root_section_reloc_target);

    let mut next_custom_section: u32 = 0;
    let mut elf_section_to_target = |elf_section_index: ElfSectionIndex| {
        *section_targets.entry(elf_section_index).or_insert_with(|| {
            let next = SectionIndex::from_u32(next_custom_section);
            section_to_custom_section.insert(elf_section_index, next);
            let target = RelocationTarget::CustomSection(next);
            next_custom_section += 1;
            target
        })
    };

    let section_bytes = |elf_section_index: ElfSectionIndex| {
        let elf_section_index = elf_section_index.as_usize();
        let byte_range = elf.section_headers[elf_section_index].file_range();
        contents[byte_range.start..byte_range.end].to_vec()
    };

    // From elf section index to list of Relocations. Although we use a Vec,
    // the order of relocations is not important.
    let mut relocations: HashMap<ElfSectionIndex, Vec<Relocation>> = HashMap::new();

    // Each iteration of this loop pulls a section and the relocations
    // relocations that apply to it. We begin with the ".root_section"
    // section, and then parse all relocation sections that apply to that
    // section. Those relocations may refer to additional sections which we
    // then add to the worklist until we've visited the closure of
    // everything needed to run the code in ".root_section".
    //
    // `worklist` is the list of sections we have yet to visit. It never
    // contains any duplicates or sections we've already visited. `visited`
    // contains all the sections we've ever added to the worklist in a set
    // so that we can quickly check whether a section is new before adding
    // it to worklist. `section_to_custom_section` is filled in with all
    // the sections we want to include.
    worklist.push(root_section_index);
    visited.insert(root_section_index);

    // Also add any .eh_frame sections.
    let mut eh_frame_section_indices = vec![];
    // TODO: this constant has been added to goblin, now waiting for release
    const SHT_X86_64_UNWIND: u32 = 0x7000_0001;
    for (index, shdr) in elf.section_headers.iter().enumerate() {
        if shdr.sh_type == SHT_X86_64_UNWIND {
            let index = ElfSectionIndex::from_usize(index)?;
            worklist.push(index);
            visited.insert(index);
            eh_frame_section_indices.push(index);
            // This allocates a custom section index for the ELF section.
            elf_section_to_target(index);
        }
    }

    while let Some(section_index) = worklist.pop() {
        for reloc in reloc_sections
            .get(&section_index)
            .iter()
            .flat_map(|inner| inner.iter().flat_map(|inner2| inner2.iter()))
        {
            let kind = match reloc.r_type {
                // TODO: these constants are not per-arch, we'll need to
                // make the whole match per-arch.
                goblin::elf::reloc::R_X86_64_64 => RelocationKind::Abs8,
                goblin::elf::reloc::R_X86_64_PC64 => RelocationKind::X86PCRel8,
                goblin::elf::reloc::R_X86_64_GOT64 => {
                    return Err(CompileError::Codegen(
                        "unimplemented PIC relocation R_X86_64_GOT64".into(),
                    ));
                }
                goblin::elf::reloc::R_X86_64_GOTPC64 => {
                    return Err(CompileError::Codegen(
                        "unimplemented PIC relocation R_X86_64_GOTPC64".into(),
                    ));
                }
                _ => {
                    return Err(CompileError::Codegen(format!(
                        "unknown ELF relocation {}",
                        reloc.r_type
                    )));
                }
            };
            let offset = reloc.r_offset as u32;
            let addend = reloc.r_addend.unwrap_or(0);
            let target = reloc.r_sym;
            let elf_target = elf.syms.get(target).ok_or_else(|| {
                CompileError::Codegen(format!(
                    "relocation refers to symbol {} past end of symbol table (len={})",
                    target,
                    elf.syms.len()
                ))
            })?;
            let elf_target_section = ElfSectionIndex::from_usize(elf_target.st_shndx)?;
            let reloc_target = if elf_target_section == root_section_index {
                root_section_reloc_target
            } else if elf_target.st_type() == goblin::elf::sym::STT_SECTION {
                if visited.insert(elf_target_section) {
                    worklist.push(elf_target_section);
                }
                elf_section_to_target(elf_target_section)
            } else if elf_target.st_type() == goblin::elf::sym::STT_NOTYPE
                && elf_target_section.is_undef()
            {
                // Not defined in this .o file. Maybe another local function?
                let name = elf_target.st_name;
                let name = elf.strtab.get(name).unwrap().unwrap().into();
                if let Some(reloc_target) = symbol_name_to_relocation_target(&name)? {
                    reloc_target
                // Maybe a libcall then?
                } else if let Some(libcall) = libcalls.get(&name) {
                    RelocationTarget::LibCall(*libcall)
                } else {
                    unimplemented!("reference to unknown symbol {}", name);
                }
            } else {
                unimplemented!("unknown relocation {:?} with target {:?}", reloc, target);
            };
            relocations
                .entry(section_index)
                .or_default()
                .push(Relocation {
                    kind,
                    reloc_target,
                    offset,
                    addend,
                });
        }
    }

    let eh_frame_section_indices = eh_frame_section_indices
        .iter()
        .map(|index| {
            section_to_custom_section.get(index).map_or_else(
                || {
                    Err(CompileError::Codegen(format!(
                        ".eh_frame section with index={:?} was never loaded",
                        index
                    )))
                },
                |idx| Ok(*idx),
            )
        })
        .collect::<Result<Vec<SectionIndex>, _>>()?;

    let mut custom_sections = section_to_custom_section
        .iter()
        .map(|(elf_section_index, custom_section_index)| {
            (
                custom_section_index,
                CustomSection {
                    protection: CustomSectionProtection::Read,
                    bytes: SectionBody::new_with_vec(section_bytes(*elf_section_index)),
                    relocations: relocations
                        .remove_entry(elf_section_index)
                        .map_or(vec![], |(_, v)| v),
                },
            )
        })
        .collect::<Vec<_>>();
    custom_sections.sort_unstable_by_key(|a| a.0);
    let custom_sections = custom_sections
        .into_iter()
        .map(|(_, v)| v)
        .collect::<PrimaryMap<SectionIndex, _>>();

    let function_body = FunctionBody {
        body: section_bytes(root_section_index),
        unwind_info: None,
    };

    let address_map = FunctionAddressMap {
        instructions: vec![InstructionAddressMap {
            srcloc: SourceLoc::default(),
            code_offset: 0,
            code_len: function_body.body.len(),
        }],
        start_srcloc: SourceLoc::default(),
        end_srcloc: SourceLoc::default(),
        body_offset: 0,
        body_len: function_body.body.len(),
    };

    Ok(CompiledFunction {
        compiled_function: wasmer_compiler::CompiledFunction {
            body: function_body,
            jt_offsets: SecondaryMap::new(),
            relocations: relocations
                .remove_entry(&root_section_index)
                .map_or(vec![], |(_, v)| v),
            frame_info: CompiledFunctionFrameInfo {
                address_map,
                traps: vec![],
            },
        },
        custom_sections,
        eh_frame_section_indices,
    })
}
