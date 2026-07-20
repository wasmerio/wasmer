//! Helpers shared by the object-based compiler backends (Cranelift and
//! Singlepass) for emitting per-function relocatable ELF objects and linking
//! them into the final module image.

use crate::compiler::{CompiledObjects, emit_metadata_and_link};
use crate::dwarf::{EhRelocation, EhTarget};
use crate::misc::{CompiledFunctionExt, CompiledKind};
use crate::object::get_object_for_target;
use crate::types::function::{Compilation, FunctionBody};
use crate::types::relocation::{Relocation, RelocationKind, RelocationTarget};
use crate::types::section::CustomSection;
use object::{
    RelocationEncoding, RelocationFlags, RelocationKind as ObjectRelocationKind, SectionKind,
    SymbolFlags, SymbolKind, SymbolScope, elf,
    write::{
        Object, Relocation as ObjectRelocation, SectionId, StandardSection, StandardSegment,
        Symbol, SymbolId, SymbolSection,
    },
};
use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use wasmer_types::{CompileError, LibCall, LocalFunctionIndex, TrapInformation, target::Target};
use wasmer_types::{FunctionIndex, FunctionType};

/// The result of compiling a single unit (function or trampoline): either an
/// in-memory body for the classic artifact format, or a relocatable object
/// file on disk for the ELF artifact format (together with the maximum stack
/// usage, when known).
pub enum CompileOutput<T> {
    /// The compiled body, kept in memory.
    InMemory(T),
    /// Path to the emitted relocatable object file and the unit's maximum
    /// stack usage, when known.
    Object(PathBuf, Option<usize>),
}

impl<T: crate::compiler::CompiledFunction> crate::compiler::CompiledFunction for CompileOutput<T> {}

/// Write a finished object into `build_directory` under `filename`.
pub fn save_object(
    object: Object<'static>,
    build_directory: &Path,
    filename: String,
) -> Result<PathBuf, CompileError> {
    let path = build_directory.join(filename);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| {
            CompileError::Codegen(format!("failed to create object {}: {e}", path.display()))
        })?;
    object.write_stream(&mut file).map_err(|e| {
        CompileError::Codegen(format!("failed to write object {}: {e}", path.display()))
    })?;
    Ok(path)
}

/// Declare an undefined text symbol resolved when the objects are linked.
pub fn add_undefined_symbol(object: &mut Object<'static>, name: String) -> SymbolId {
    object.add_symbol(Symbol {
        name: name.into_bytes(),
        value: 0,
        size: 0,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    })
}

/// Declare an undefined dynamic symbol for a libcall, resolved by the runtime
/// loader through a dynamic relocation.
pub fn add_libcall_symbol(object: &mut Object<'static>, libcall: LibCall) -> SymbolId {
    object.add_symbol(Symbol {
        name: libcall.to_function_name().to_string().into_bytes(),
        value: 0,
        size: 0,
        kind: SymbolKind::Unknown,
        scope: SymbolScope::Dynamic,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    })
}

/// Map a Wasmer relocation kind onto the corresponding object-file relocation
/// flags.
pub fn relocation_kind_to_flags(kind: RelocationKind) -> Result<RelocationFlags, CompileError> {
    use ObjectRelocationKind as K;
    Ok(match kind {
        RelocationKind::Abs4 => RelocationFlags::Generic {
            kind: K::Absolute,
            encoding: RelocationEncoding::Generic,
            size: 32,
        },
        RelocationKind::Abs8 => RelocationFlags::Generic {
            kind: K::Absolute,
            encoding: RelocationEncoding::Generic,
            size: 64,
        },
        RelocationKind::PCRel4 => RelocationFlags::Generic {
            kind: K::Relative,
            encoding: RelocationEncoding::Generic,
            size: 32,
        },
        RelocationKind::X86CallPCRel4 => RelocationFlags::Generic {
            kind: K::Relative,
            encoding: RelocationEncoding::X86Branch,
            size: 32,
        },
        RelocationKind::X86CallPLTRel4 => RelocationFlags::Generic {
            kind: K::PltRelative,
            encoding: RelocationEncoding::X86Branch,
            size: 32,
        },
        RelocationKind::X86GOTPCRel4 => RelocationFlags::Generic {
            kind: K::GotRelative,
            encoding: RelocationEncoding::Generic,
            size: 32,
        },
        RelocationKind::Arm64Call => RelocationFlags::Elf {
            r_type: elf::R_AARCH64_CALL26,
        },
        // For RISC-V relocations, please refer to:
        // https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/2484f950a551c653f1823f1bd11926bf5a57fae3/riscv-elf.adoc#relocations
        RelocationKind::RiscvPCRelHi20 => RelocationFlags::Elf {
            r_type: elf::R_RISCV_PCREL_HI20,
        },
        RelocationKind::RiscvPCRelLo12I => RelocationFlags::Elf {
            r_type: elf::R_RISCV_PCREL_LO12_I,
        },
        RelocationKind::RiscvCall => RelocationFlags::Elf {
            r_type: elf::R_RISCV_CALL_PLT,
        },
        kind => {
            return Err(CompileError::Codegen(format!(
                "unsupported ELF relocation kind: {kind:?}"
            )));
        }
    })
}

/// Apply the compiled code's relocations to `section`, declaring the referenced symbols.
pub fn add_relocations(
    object: &mut Object<'static>,
    section: SectionId,
    relocations: &[Relocation],
    local_symbol: Option<(LocalFunctionIndex, SymbolId)>,
) -> Result<(), CompileError> {
    for relocation in relocations {
        let symbol = match relocation.reloc_target {
            RelocationTarget::LocalFunc(index) => local_symbol
                .filter(|(local_index, _)| *local_index == index)
                .map_or_else(
                    || {
                        add_undefined_symbol(
                            object,
                            CompiledKind::Local(index, String::new()).linkage_name(),
                        )
                    },
                    |(_, symbol)| symbol,
                ),
            RelocationTarget::CustomSection(index) => add_undefined_symbol(
                object,
                CompiledKind::ImportFunctionTrampoline(
                    FunctionIndex::from_u32(index.as_u32()),
                    FunctionType::default(),
                )
                .linkage_name(),
            ),
            RelocationTarget::LibCall(libcall) => add_libcall_symbol(object, libcall),
            RelocationTarget::DynamicTrampoline(index) => add_undefined_symbol(
                object,
                CompiledKind::DynamicFunctionTrampoline(index, FunctionType::default())
                    .linkage_name(),
            ),
        };
        let flags = relocation_kind_to_flags(relocation.kind)?;
        object
            .add_relocation(
                section,
                ObjectRelocation {
                    offset: relocation.offset as u64,
                    flags,
                    symbol,
                    addend: relocation.addend,
                },
            )
            .map_err(|e| CompileError::Codegen(format!("failed to add ELF relocation: {e}")))?;
    }
    Ok(())
}

/// Emit the per-function trap table into a `.w.traps` section, under a weak
/// data symbol so the metadata object can reference it per function.
pub fn emit_trap_section(
    object: &mut Object<'static>,
    kind: &CompiledKind,
    traps: &[TrapInformation],
) {
    let mut trap_data = Vec::with_capacity(traps.len() * 8 + size_of::<u32>());
    trap_data.extend_from_slice(&(traps.len() as u32).to_le_bytes());
    for trap in traps {
        trap_data.extend_from_slice(&trap.code_offset.to_le_bytes());
        trap_data.extend_from_slice(&(trap.trap_code as u32).to_le_bytes());
    }
    let traps_section = object.add_section(
        object.segment_name(StandardSegment::Data).to_vec(),
        crate::WASMER_TRAPS_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    let traps_symbol = object.add_symbol(Symbol {
        name: kind.traps_name().into_bytes(),
        value: 0,
        size: trap_data.len() as u64,
        kind: SymbolKind::Data,
        scope: SymbolScope::Linkage,
        weak: true,
        section: SymbolSection::Section(traps_section),
        flags: SymbolFlags::None,
    });
    object.add_symbol_data(traps_symbol, traps_section, &trap_data, 4);
}

/// Emit a serialized `.eh_frame` blob into its own section, resolving the
/// recorded relocations against the function's text symbol, the exception
/// personality routine and the function's LSDA section.
pub fn emit_eh_frame_section(
    object: &mut Object<'static>,
    eh_frame_bytes: &[u8],
    relocations: &[EhRelocation],
    function_symbol: SymbolId,
    lsda_section_symbol: Option<SymbolId>,
) -> Result<(), CompileError> {
    let section = object.add_section(
        object.segment_name(StandardSegment::Debug).to_vec(),
        crate::EH_FRAME_SECTION_NAME.to_vec(),
        SectionKind::Other,
    );
    let data_offset = object.append_section_data(section, eh_frame_bytes, 4);

    // The personality symbol is added lazily, the first time a relocation
    // references it.
    let mut personality_symbol = None;
    for relocation in relocations {
        let symbol = match relocation.target {
            EhTarget::Function => function_symbol,
            EhTarget::Personality => *personality_symbol
                .get_or_insert_with(|| add_libcall_symbol(object, LibCall::EHPersonality)),
            EhTarget::Lsda => lsda_section_symbol.ok_or_else(|| {
                CompileError::Codegen(
                    ".eh_frame references an LSDA but none was emitted".to_string(),
                )
            })?,
        };
        object
            .add_relocation(
                section,
                ObjectRelocation {
                    offset: data_offset + relocation.offset,
                    flags: RelocationFlags::Generic {
                        kind: relocation.kind,
                        encoding: RelocationEncoding::Generic,
                        size: 8 * relocation.size,
                    },
                    symbol,
                    addend: relocation.addend,
                },
            )
            .map_err(|e| {
                CompileError::Codegen(format!("failed to add .eh_frame relocation: {e}"))
            })?;
    }
    Ok(())
}

/// Serialize a trampoline's body into its own relocatable object file.
pub fn emit_function_body(
    target: &Target,
    build_directory: &Path,
    kind: &CompiledKind,
    body: &FunctionBody,
) -> Result<PathBuf, CompileError> {
    let mut object = get_object_for_target(target.triple())
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
    let symbol = object.add_symbol(Symbol {
        name: kind.linkage_name().into_bytes(),
        value: 0,
        size: body.body.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let text = object.section_id(StandardSection::Text);
    object.add_symbol_data(symbol, text, &body.body, 4);
    save_object(object, build_directory, kind.object_filename())
}

/// Serialize an import trampoline's custom section into its own relocatable object file.
pub fn emit_import_trampoline(
    target: &Target,
    build_directory: &Path,
    kind: &CompiledKind,
    section: &CustomSection,
) -> Result<PathBuf, CompileError> {
    let mut object = get_object_for_target(target.triple())
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
    let symbol = object.add_symbol(Symbol {
        name: kind.linkage_name().into_bytes(),
        value: 0,
        size: section.bytes.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let text = object.section_id(StandardSection::Text);
    object.add_symbol_data(symbol, text, section.bytes.as_slice(), 4);
    save_object(object, build_directory, kind.object_filename())
}

/// Link all the per-function and trampoline objects (plus the Wasmer metadata
/// object) into the final shared-object module image.
#[allow(clippy::too_many_arguments)]
pub fn link_module(
    target: &Target,
    compile_info_blob: &[u8],
    build_directory: &Path,
    object_files: &[PathBuf],
    import_trampoline_objects: &[PathBuf],
    trampoline_objects: &[PathBuf],
    dynamic_trampoline_objects: &[PathBuf],
    debug_dir: Option<PathBuf>,
    module_hash: Option<String>,
) -> Result<Compilation, CompileError> {
    let module_file = NamedTempFile::new_in(build_directory)
        .map_err(|e| CompileError::Codegen(format!("cannot create temporary module file: {e}")))?;
    let mut module_file = emit_metadata_and_link(
        target,
        compile_info_blob,
        build_directory,
        module_file,
        &CompiledObjects {
            object_files,
            import_trampoline_object_files: import_trampoline_objects,
            trampoline_object_files: trampoline_objects,
            dynamic_trampoline_object_files: dynamic_trampoline_objects,
        },
        debug_dir,
        module_hash,
    )?;
    let mut elf = Vec::new();
    module_file
        .read_to_end(&mut elf)
        .map_err(|e| CompileError::Codegen(format!("cannot read linked module artifact: {e}")))?;
    Ok(Compilation::Elf(elf))
}
