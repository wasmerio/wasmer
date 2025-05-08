use object::{Object, ObjectSection, ObjectSymbol};
use target_lexicon::BinaryFormat;

use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::num::TryFromIntError;

use wasmer_types::{entity::PrimaryMap, CompileError, SourceLoc};

use wasmer_compiler::types::{
    address_map::{FunctionAddressMap, InstructionAddressMap},
    function::{CompiledFunctionFrameInfo, CustomSections, FunctionBody},
    relocation::{Relocation, RelocationKind, RelocationTarget},
    section::{CustomSection, CustomSectionProtection, SectionBody, SectionIndex},
};

use wasmer_vm::libcalls::LibCall;

fn map_tryfromint_err(error: TryFromIntError) -> CompileError {
    CompileError::Codegen(format!("int doesn't fit: {error}"))
}

fn map_object_err(error: object::read::Error) -> CompileError {
    CompileError::Codegen(format!("error parsing object file: {error}"))
}

#[derive(Debug)]
pub struct CompiledFunction {
    pub compiled_function: wasmer_compiler::types::function::CompiledFunction,
    pub custom_sections: CustomSections,
    pub eh_frame_section_indices: Vec<SectionIndex>,
    pub compact_unwind_section_indices: Vec<SectionIndex>,
}

static LIBCALLS_ELF: phf::Map<&'static str, LibCall> = phf::phf_map! {
    "ceilf" => LibCall::CeilF32,
    "ceil" => LibCall::CeilF64,
    "floorf" => LibCall::FloorF32,
    "floor" => LibCall::FloorF64,
    "nearbyintf" => LibCall::NearestF32,
    "nearbyint" => LibCall::NearestF64,
    "truncf" => LibCall::TruncF32,
    "trunc" => LibCall::TruncF64,
    "wasmer_vm_f32_ceil" => LibCall::CeilF32,
    "wasmer_vm_f64_ceil" => LibCall::CeilF64,
    "wasmer_vm_f32_floor" => LibCall::FloorF32,
    "wasmer_vm_f64_floor" => LibCall::FloorF64,
    "wasmer_vm_f32_nearest" => LibCall::NearestF32,
    "wasmer_vm_f64_nearest" => LibCall::NearestF64,
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
    "wasmer_vm_rethrow" => LibCall::Rethrow,
    "wasmer_vm_alloc_exception" => LibCall::AllocException,
    "wasmer_vm_delete_exception" => LibCall::DeleteException,
    "wasmer_vm_read_exception" => LibCall::ReadException,
    "wasmer_vm_dbg_usize" => LibCall::DebugUsize,
    "wasmer_eh_personality" => LibCall::EHPersonality,
    "wasmer_vm_dbg_str" => LibCall::DebugStr,
};

static LIBCALLS_MACHO: phf::Map<&'static str, LibCall> = phf::phf_map! {
    "_ceilf" => LibCall::CeilF32,
    "_ceil" => LibCall::CeilF64,
    "_floorf" => LibCall::FloorF32,
    "_floor" => LibCall::FloorF64,
    "_nearbyintf" => LibCall::NearestF32,
    "_nearbyint" => LibCall::NearestF64,
    "_truncf" => LibCall::TruncF32,
    "_trunc" => LibCall::TruncF64,
    "_wasmer_vm_f32_ceil" => LibCall::CeilF32,
    "_wasmer_vm_f64_ceil" => LibCall::CeilF64,
    "_wasmer_vm_f32_floor" => LibCall::FloorF32,
    "_wasmer_vm_f64_floor" => LibCall::FloorF64,
    "_wasmer_vm_f32_nearest" => LibCall::NearestF32,
    "_wasmer_vm_f64_nearest" => LibCall::NearestF64,
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
    "_wasmer_vm_rethrow" => LibCall::Rethrow,
    "_wasmer_vm_alloc_exception" => LibCall::AllocException,
    "_wasmer_vm_delete_exception" => LibCall::DeleteException,
    "_wasmer_vm_read_exception" => LibCall::ReadException,
    "_wasmer_vm_dbg_usize" => LibCall::DebugUsize,
    // Note: on macOS+Mach-O the personality function *must* be called like this, otherwise LLVM
    // will generate things differently than "normal", wreaking havoc.
    //
    // todo: find out if it is a bug in LLVM or it is expected.
    "___gxx_personality_v0" => LibCall::EHPersonality,
    "_wasmer_vm_dbg_str" => LibCall::DebugStr,
};

pub fn load_object_file<F>(
    contents: &[u8],
    root_section: &str,
    root_section_reloc_target: RelocationTarget,
    mut symbol_name_to_relocation_target: F,
    binary_fmt: BinaryFormat,
) -> Result<CompiledFunction, CompileError>
where
    F: FnMut(&str) -> Result<Option<RelocationTarget>, CompileError>,
{
    let obj = object::File::parse(contents).map_err(map_object_err)?;

    let libcalls = match binary_fmt {
        BinaryFormat::Elf => &LIBCALLS_ELF,
        BinaryFormat::Macho => &LIBCALLS_MACHO,
        _ => {
            return Err(CompileError::UnsupportedTarget(format!(
                "Unsupported binary format {binary_fmt:?}"
            )))
        }
    };

    let mut visited: HashSet<object::read::SectionIndex> = HashSet::new();
    let mut worklist: Vec<object::read::SectionIndex> = Vec::new();
    let mut section_targets: HashMap<object::read::SectionIndex, RelocationTarget> = HashMap::new();

    let root_section_index = obj
        .section_by_name(root_section)
        .ok_or_else(|| CompileError::Codegen(format!("no section named {root_section}")))?
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
    // that apply to it. We begin with the ".root_section"
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

    // Add any .eh_frame sections.
    let mut eh_frame_section_indices = vec![];

    // Add macos-specific unwind sections.
    let mut compact_unwind_section_indices = vec![];

    for section in obj.sections() {
        let index = section.index();
        if section.kind() == object::SectionKind::Elf(object::elf::SHT_X86_64_UNWIND)
            || section.name().unwrap_or_default() == "__eh_frame"
        {
            worklist.push(index);
            eh_frame_section_indices.push(index);

            // This allocates a custom section index for the ELF section.
            elf_section_to_target(index);
        } else if section.name().unwrap_or_default() == "__compact_unwind" {
            worklist.push(index);
            compact_unwind_section_indices.push(index);

            elf_section_to_target(index);
        }
    }

    while let Some(section_index) = worklist.pop() {
        let sec = obj
            .section_by_index(section_index)
            .map_err(map_object_err)?;
        let relocs = sec.relocations();
        for (offset, reloc) in relocs {
            let mut addend = reloc.addend();
            let target = match reloc.target() {
                object::read::RelocationTarget::Symbol(index) => {
                    let symbol = obj.symbol_by_index(index).map_err(map_object_err)?;
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
                                    "relocation targets unknown section {reloc:?}",
                                )));
                            }
                        }
                        // Maybe a libcall then?
                    } else if let Some(libcall) = libcalls.get(symbol_name) {
                        RelocationTarget::LibCall(*libcall)
                    } else if let Ok(Some(reloc_target)) =
                        symbol_name_to_relocation_target(symbol_name)
                    {
                        reloc_target
                    } else if let object::SymbolSection::Section(section_index) = symbol.section() {
                        if matches!(
                            reloc.kind(),
                            object::RelocationKind::MachO {
                                value: object::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12,
                                relative: false
                            } | object::RelocationKind::MachO {
                                value: object::macho::ARM64_RELOC_POINTER_TO_GOT,
                                relative: true
                            } | object::RelocationKind::MachO {
                                value: object::macho::ARM64_RELOC_GOT_LOAD_PAGE21,
                                relative: true
                            } | object::RelocationKind::MachO {
                                value: object::macho::ARM64_RELOC_PAGE21,
                                relative: true
                            } | object::RelocationKind::MachO {
                                value: object::macho::ARM64_RELOC_PAGEOFF12,
                                relative: false
                            }
                        ) {
                            // (caveat: this comment comes from a point in time after the `addend`
                            // math in the else branch)
                            //
                            // (caveat2: this is mach-o + aarch64 only)
                            //
                            // The tampering with the addend in the else branch causes some
                            // problems with GOT-based relocs, as a non-zero addend has no meaning
                            // when dealing with GOT entries, for our use-case.
                            //
                            // However, for some reasons, it happens that we conceptually need to
                            // have relocations that pretty much mean "the contents of this GOT
                            // entry plus a non-zero addend". When in this case, we will later
                            // perform what is known as "GOT relaxation", i.e. we can change the
                            // `ldr` opcode to an `add`.
                            //
                            // For this to make sense we need to fix the addend to be the delta
                            // between the section whose address is an entry of the GOT and the
                            // symbol that is the target of the relocation.

                            let symbol_sec = obj
                                .section_by_index(section_index)
                                .map_err(map_object_err)?;

                            addend = addend
                                .wrapping_add((symbol.address() - symbol_sec.address()) as i64);
                        } else {
                            // TODO: Encode symbol address into addend, I think this is a bit hacky.
                            addend = addend.wrapping_add(symbol.address() as i64);
                        }

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
                            "relocation {reloc:?} targets unknown symbol '{symbol:?}'",
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
                        "relocation targets absolute address {reloc:?}",
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
                        "relocation target is unknown `{t:?}`",
                    )));
                }
            };
            let kind = match (obj.architecture(), reloc.kind(), reloc.size()) {
                (_, object::RelocationKind::Absolute, 64) => RelocationKind::Abs8,
                (_, object::RelocationKind::Absolute, 32) => RelocationKind::Abs4,
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
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_ABS_HI20),
                    0,
                ) => RelocationKind::LArchAbsHi20,
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_ABS_LO12),
                    0,
                ) => RelocationKind::LArchAbsLo12,
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_ABS64_HI12),
                    0,
                ) => RelocationKind::LArchAbs64Hi12,
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_ABS64_LO20),
                    0,
                ) => RelocationKind::LArchAbs64Lo20,
                (
                    object::Architecture::LoongArch64,
                    // FIXME: Replace with R_LARCH_CALL36 while object is updated
                    // to 0.32.2.
                    // https://github.com/gimli-rs/object/commit/16b6d902f6c9b39ec7aaea141460f8981e57dd79
                    object::RelocationKind::Elf(110),
                    0,
                ) => RelocationKind::LArchCall36,
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_PCALA_HI20),
                    0,
                ) => RelocationKind::LArchPCAlaHi20,
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_PCALA_LO12),
                    0,
                ) => RelocationKind::LArchPCAlaLo12,
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_PCALA64_HI12),
                    0,
                ) => RelocationKind::LArchPCAla64Hi12,
                (
                    object::Architecture::LoongArch64,
                    object::RelocationKind::Elf(object::elf::R_LARCH_PCALA64_LO20),
                    0,
                ) => RelocationKind::LArchPCAla64Lo20,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_ADR_PREL_LO21),
                    0,
                ) => RelocationKind::Aarch64AdrPrelLo21,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_ADR_PREL_PG_HI21),
                    0,
                ) => RelocationKind::Aarch64AdrPrelPgHi21,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_LDST128_ABS_LO12_NC),
                    0,
                ) => RelocationKind::Aarch64Ldst128AbsLo12Nc,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_ADD_ABS_LO12_NC),
                    0,
                ) => RelocationKind::Aarch64AddAbsLo12Nc,
                (
                    object::Architecture::Aarch64,
                    object::RelocationKind::Elf(object::elf::R_AARCH64_LDST64_ABS_LO12_NC),
                    0,
                ) => RelocationKind::Aarch64Ldst64AbsLo12Nc,
                (object::Architecture::Aarch64, object::RelocationKind::MachO { value, .. }, _) => {
                    match value {
                        object::macho::ARM64_RELOC_UNSIGNED => {
                            RelocationKind::MachoArm64RelocUnsigned
                        }
                        object::macho::ARM64_RELOC_SUBTRACTOR => {
                            RelocationKind::MachoArm64RelocSubtractor
                        }
                        object::macho::ARM64_RELOC_BRANCH26 => {
                            RelocationKind::MachoArm64RelocBranch26
                        }
                        object::macho::ARM64_RELOC_PAGE21 => RelocationKind::MachoArm64RelocPage21,
                        object::macho::ARM64_RELOC_PAGEOFF12 => {
                            RelocationKind::MachoArm64RelocPageoff12
                        }
                        object::macho::ARM64_RELOC_GOT_LOAD_PAGE21 => {
                            RelocationKind::MachoArm64RelocGotLoadPage21
                        }
                        object::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12 => {
                            RelocationKind::MachoArm64RelocGotLoadPageoff12
                        }
                        object::macho::ARM64_RELOC_POINTER_TO_GOT => {
                            RelocationKind::MachoArm64RelocPointerToGot
                        }
                        object::macho::ARM64_RELOC_TLVP_LOAD_PAGE21 => {
                            RelocationKind::MachoArm64RelocTlvpLoadPage21
                        }
                        object::macho::ARM64_RELOC_TLVP_LOAD_PAGEOFF12 => {
                            RelocationKind::MachoArm64RelocTlvpLoadPageoff12
                        }
                        object::macho::ARM64_RELOC_ADDEND => RelocationKind::MachoArm64RelocAddend,
                        _ => {
                            return Err(CompileError::Codegen(format!(
                                "unknown relocation {reloc:?}",
                            )))
                        }
                    }
                }
                (object::Architecture::X86_64, object::RelocationKind::MachO { value, .. }, _) => {
                    match value {
                        object::macho::X86_64_RELOC_UNSIGNED => {
                            RelocationKind::MachoX86_64RelocUnsigned
                        }
                        object::macho::X86_64_RELOC_SIGNED => {
                            RelocationKind::MachoX86_64RelocSigned
                        }
                        object::macho::X86_64_RELOC_BRANCH => {
                            RelocationKind::MachoX86_64RelocBranch
                        }
                        object::macho::X86_64_RELOC_GOT_LOAD => {
                            RelocationKind::MachoX86_64RelocGotLoad
                        }
                        object::macho::X86_64_RELOC_GOT => RelocationKind::MachoX86_64RelocGot,
                        object::macho::X86_64_RELOC_SUBTRACTOR => {
                            RelocationKind::MachoX86_64RelocSubtractor
                        }
                        object::macho::X86_64_RELOC_SIGNED_1 => {
                            RelocationKind::MachoX86_64RelocSigned1
                        }
                        object::macho::X86_64_RELOC_SIGNED_2 => {
                            RelocationKind::MachoX86_64RelocSigned2
                        }
                        object::macho::X86_64_RELOC_SIGNED_4 => {
                            RelocationKind::MachoX86_64RelocSigned4
                        }
                        object::macho::X86_64_RELOC_TLV => RelocationKind::MachoX86_64RelocTlv,
                        _ => {
                            return Err(CompileError::Codegen(format!(
                                "unknown relocation {reloc:?}"
                            )))
                        }
                    }
                }
                _ => {
                    return Err(CompileError::Codegen(format!(
                        "unknown relocation {reloc:?}",
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
                        ".eh_frame section with index={index:?} was never loaded",
                    )))
                },
                |idx| Ok(*idx),
            )
        })
        .collect::<Result<Vec<SectionIndex>, _>>()?;

    let compact_unwind_section_indices = compact_unwind_section_indices
        .iter()
        .map(|index| {
            section_to_custom_section.get(index).map_or_else(
                || {
                    Err(CompileError::Codegen(format!(
                        "_compact_unwind section with index={index:?} was never loaded",
                    )))
                },
                |idx| Ok(*idx),
            )
        })
        .collect::<Result<Vec<SectionIndex>, _>>()?;

    let mut custom_sections = section_to_custom_section
        .iter()
        .map(|(elf_section_index, custom_section_index)| {
            let section = obj.section_by_index(*elf_section_index).unwrap();
            (
                custom_section_index,
                CustomSection {
                    protection: CustomSectionProtection::Read,
                    alignment: Some(section.align()),
                    bytes: SectionBody::new_with_vec(section.data().unwrap().to_vec()),
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
        body: obj
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
        compiled_function: wasmer_compiler::types::function::CompiledFunction {
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
        compact_unwind_section_indices,
    })
}
