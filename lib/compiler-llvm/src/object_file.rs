use object::{Object, ObjectSection, ObjectSymbol};

use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::num::TryFromIntError;

use wasmer_types::entity::PrimaryMap;
use wasmer_types::{
    CompileError, CompiledFunctionFrameInfo, CustomSection, CustomSectionProtection,
    CustomSections, FunctionAddressMap, FunctionBody, InstructionAddressMap, Relocation,
    RelocationKind, RelocationTarget, SectionBody, SectionIndex, SourceLoc,
};
use wasmer_vm::libcalls::LibCall;

fn map_tryfromint_err(error: TryFromIntError) -> CompileError {
    CompileError::Codegen(format!("int doesn't fit: {}", error))
}

fn map_object_err(error: object::read::Error) -> CompileError {
    CompileError::Codegen(format!("error parsing object file: {}", error))
}

pub struct CompiledFunction {
    pub compiled_function: wasmer_types::CompiledFunction,
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
    F: FnMut(&str) -> Result<Option<RelocationTarget>, CompileError>,
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
    libcalls.insert(
        "wasmer_vm_memory32_atomic_wait32".to_string(),
        LibCall::Memory32AtomicWait32,
    );
    libcalls.insert(
        "wasmer_vm_imported_memory32_atomic_wait32".to_string(),
        LibCall::ImportedMemory32AtomicWait32,
    );
    libcalls.insert(
        "wasmer_vm_memory32_atomic_wait64".to_string(),
        LibCall::Memory32AtomicWait64,
    );
    libcalls.insert(
        "wasmer_vm_imported_memory32_atomic_wait64".to_string(),
        LibCall::ImportedMemory32AtomicWait64,
    );
    libcalls.insert(
        "wasmer_vm_memory32_atomic_notify".to_string(),
        LibCall::Memory32AtomicNotify,
    );
    libcalls.insert(
        "wasmer_vm_imported_memory32_atomic_notify".to_string(),
        LibCall::ImportedMemory32AtomicNotify,
    );

    let elf = object::File::parse(contents).map_err(map_object_err)?;

    let mut visited: HashSet<object::read::SectionIndex> = HashSet::new();
    let mut worklist: Vec<object::read::SectionIndex> = Vec::new();
    let mut section_targets: HashMap<object::read::SectionIndex, RelocationTarget> = HashMap::new();

    let root_section_index = elf
        .section_by_name(root_section)
        .ok_or_else(|| CompileError::Codegen(format!("no section named {}", root_section)))?
        .index();

    let mut section_to_custom_section = HashMap::new();

    section_targets.insert(root_section_index, root_section_reloc_target);

    let mut next_custom_section: u32 = 0;
    let mut elf_section_to_target = |elf_section_index: object::read::SectionIndex| {
        *section_targets.entry(elf_section_index).or_insert_with(|| {
            let next = SectionIndex::from_u32(next_custom_section);
            section_to_custom_section.insert(elf_section_index, next);
            let target = RelocationTarget::CustomSection(next);
            next_custom_section += 1;
            target
        })
    };

    // From elf section index to list of Relocations. Although we use a Vec,
    // the order of relocations is not important.
    let mut relocations: HashMap<object::read::SectionIndex, Vec<Relocation>> = HashMap::new();

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
    for section in elf.sections() {
        if section.kind() == object::SectionKind::Elf(object::elf::SHT_X86_64_UNWIND) {
            let index = section.index();
            worklist.push(index);
            visited.insert(index);
            eh_frame_section_indices.push(index);
            // This allocates a custom section index for the ELF section.
            elf_section_to_target(index);
        }
    }

    while let Some(section_index) = worklist.pop() {
        for (offset, reloc) in elf
            .section_by_index(section_index)
            .map_err(map_object_err)?
            .relocations()
        {
            let kind = match (elf.architecture(), reloc.kind(), reloc.size()) {
                (_, object::RelocationKind::Absolute, 64) => RelocationKind::Abs8,
                (
                    object::Architecture::X86_64,
                    object::RelocationKind::Elf(object::elf::R_X86_64_PC64),
                    0,
                ) => RelocationKind::X86PCRel8,
                (object::Architecture::Aarch64, object::RelocationKind::PltRelative, 26) => {
                    RelocationKind::Arm64Call
                }
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_MOVW_UABS_G0_NC),
                    0,
                ) => RelocationKind::Arm64Movw0,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_MOVW_UABS_G1_NC),
                    0,
                ) => RelocationKind::Arm64Movw1,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_MOVW_UABS_G2_NC),
                    0,
                ) => RelocationKind::Arm64Movw2,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_MOVW_UABS_G3),
                    0,
                ) => RelocationKind::Arm64Movw3,
                (
                    object::Architecture::Riscv64,
                    object::RelocationKind::Elf(object::elf::R_RISCV_CALL_PLT),
                    0,
                ) => RelocationKind::RiscvCall,
                (
                    object::Architecture::Riscv64,
                    object::RelocationKind::Elf(object::elf::R_RISCV_PCREL_HI20),
                    0,
                ) => RelocationKind::RiscvPCRelHi20,
                (
                    object::Architecture::Riscv64,
                    object::RelocationKind::Elf(object::elf::R_RISCV_PCREL_LO12_I),
                    0,
                ) => RelocationKind::RiscvPCRelLo12I,
                _ => {
                    return Err(CompileError::Codegen(format!(
                        "unknown relocation {:?}",
                        reloc
                    )));
                }
            };
            let mut addend = reloc.addend();
            let target = match reloc.target() {
                object::read::RelocationTarget::Symbol(index) => {
                    let symbol = elf.symbol_by_index(index).map_err(map_object_err)?;
                    let symbol_name = symbol.name().map_err(map_object_err)?;
                    if symbol.kind() == object::SymbolKind::Section {
                        match symbol.section() {
                            object::SymbolSection::Section(section_index) => {
                                if section_index == root_section_index {
                                    root_section_reloc_target
                                } else {
                                    if visited.insert(section_index) {
                                        worklist.push(section_index);
                                    }
                                    elf_section_to_target(section_index)
                                }
                            }
                            _ => {
                                return Err(CompileError::Codegen(format!(
                                    "relocation targets unknown section {:?}",
                                    reloc
                                )));
                            }
                        }
                        // Maybe a libcall then?
                    } else if let Some(libcall) = libcalls.get(symbol_name) {
                        RelocationTarget::LibCall(*libcall)
                    } else if let Some(reloc_target) =
                        symbol_name_to_relocation_target(symbol_name)?
                    {
                        reloc_target
                    } else if let object::SymbolSection::Section(section_index) = symbol.section() {
                        // TODO: Encode symbol address into addend, I think this is a bit hacky.
                        addend = addend.wrapping_add(symbol.address() as i64);

                        if section_index == root_section_index {
                            root_section_reloc_target
                        } else {
                            if visited.insert(section_index) {
                                worklist.push(section_index);
                            }

                            elf_section_to_target(section_index)
                        }
                    } else {
                        return Err(CompileError::Codegen(format!(
                            "relocation targets unknown symbol {:?}",
                            reloc
                        )));
                    }
                }

                object::read::RelocationTarget::Section(index) => {
                    if index == root_section_index {
                        root_section_reloc_target
                    } else {
                        if visited.insert(index) {
                            worklist.push(index);
                        }
                        elf_section_to_target(index)
                    }
                }

                object::read::RelocationTarget::Absolute => {
                    // Wasm-produced object files should never have absolute
                    // addresses in them because none of the parts of the Wasm
                    // VM, nor the generated code are loaded at fixed addresses.
                    return Err(CompileError::Codegen(format!(
                        "relocation targets absolute address {:?}",
                        reloc
                    )));
                }

                // `object::read::RelocationTarget` is a
                // non-exhaustive enum (`#[non_exhaustive]`), so it
                // could have additional variants added in the
                // future. Therefore, when matching against variants
                // of non-exhaustive enums, an extra wildcard arm must
                // be added to account for any future variants.
                t => {
                    return Err(CompileError::Codegen(format!(
                        "relocation target is unknown `{:?}`",
                        t
                    )));
                }
            };
            relocations
                .entry(section_index)
                .or_default()
                .push(Relocation {
                    kind,
                    reloc_target: target,
                    offset: offset.try_into().map_err(map_tryfromint_err)?,
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
                    bytes: SectionBody::new_with_vec(
                        elf.section_by_index(*elf_section_index)
                            .unwrap()
                            .data()
                            .unwrap()
                            .to_vec(),
                    ),
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
        body: elf
            .section_by_index(root_section_index)
            .unwrap()
            .data()
            .unwrap()
            .to_vec(),
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
        compiled_function: wasmer_types::CompiledFunction {
            body: function_body,
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
