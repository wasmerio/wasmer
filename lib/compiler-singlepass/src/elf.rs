use crate::unwind::UnwindFrame;
#[cfg(feature = "unwind")]
use crate::{dwarf::WriterRelocate, unwind::create_systemv_cie};
#[cfg(feature = "unwind")]
use gimli::write::{EhFrame, FrameTable};
use object::{
    RelocationEncoding, RelocationFlags, RelocationKind as ObjectRelocationKind, SectionKind,
    SymbolFlags, SymbolKind, SymbolScope,
    write::{
        Object, Relocation as ObjectRelocation, StandardSection, StandardSegment, Symbol, SymbolId,
        SymbolSection,
    },
};
use std::{
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use wasmer_compiler::{
    CompiledObjects, WASMER_TRAPS_SECTION_NAME, emit_metadata_and_link,
    misc::{CompiledFunctionExt, CompiledKind},
    object::get_object_for_target,
    types::{
        function::{Compilation, CompiledFunction, FunctionBody},
        relocation::{Relocation, RelocationKind, RelocationTarget},
        section::CustomSection,
    },
};
use wasmer_types::{CompileError, LocalFunctionIndex, target::Target};

pub(crate) enum CompileOutput<T> {
    InMemory(T),
    Object(PathBuf),
}

fn save_object(
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
            CompileError::Codegen(format!(
                "failed to create Singlepass object {}: {e}",
                path.display()
            ))
        })?;
    object.write_stream(&mut file).map_err(|e| {
        CompileError::Codegen(format!(
            "failed to write Singlepass object {}: {e}",
            path.display()
        ))
    })?;
    Ok(path)
}

fn add_undefined_symbol(object: &mut Object<'static>, name: String) -> SymbolId {
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

fn add_relocations(
    object: &mut Object<'static>,
    section: object::write::SectionId,
    relocations: &[Relocation],
    local_symbol: Option<(LocalFunctionIndex, SymbolId)>,
) -> Result<(), CompileError> {
    for relocation in relocations {
        let symbol = match relocation.reloc_target {
            RelocationTarget::LocalFunc(index) => local_symbol
                .filter(|(local_index, _)| *local_index == index)
                .map(|(_, symbol)| symbol)
                .unwrap_or_else(|| {
                    add_undefined_symbol(
                        object,
                        CompiledKind::Local(index, String::new()).linkage_name(),
                    )
                }),
            RelocationTarget::CustomSection(index) => {
                add_undefined_symbol(object, format!("i{}", index.as_u32()))
            }
            target => {
                return Err(CompileError::Codegen(format!(
                    "unsupported Singlepass ELF relocation target: {target:?}"
                )));
            }
        };
        let flags = match relocation.kind {
            RelocationKind::Abs8 => RelocationFlags::Generic {
                kind: ObjectRelocationKind::Absolute,
                encoding: RelocationEncoding::Generic,
                size: 64,
            },
            RelocationKind::X86CallPCRel4 => RelocationFlags::Generic {
                kind: ObjectRelocationKind::Relative,
                encoding: RelocationEncoding::X86Branch,
                size: 32,
            },
            kind => {
                return Err(CompileError::Codegen(format!(
                    "unsupported Singlepass ELF relocation kind: {kind:?}"
                )));
            }
        };
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
            .map_err(|e| {
                CompileError::Codegen(format!("failed to add Singlepass ELF relocation: {e}"))
            })?;
    }
    Ok(())
}

pub(crate) fn emit_local_function(
    target: &Target,
    build_directory: &Path,
    index: LocalFunctionIndex,
    function: CompiledFunction,
    fde: Option<UnwindFrame>,
) -> Result<PathBuf, CompileError> {
    let kind = CompiledKind::Local(index, String::new());
    let mut object = get_object_for_target(target.triple())
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
    let symbol = object.add_symbol(Symbol {
        name: kind.linkage_name().into_bytes(),
        value: 0,
        size: function.body.body.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let text = object.section_id(StandardSection::Text);
    object.add_symbol_data(symbol, text, &function.body.body, 4);
    add_relocations(
        &mut object,
        text,
        &function.relocations,
        Some((index, symbol)),
    )?;

    let mut trap_data = Vec::with_capacity(function.frame_info.traps.len() * 8 + 4);
    trap_data.extend_from_slice(&(function.frame_info.traps.len() as u32).to_le_bytes());
    for trap in &function.frame_info.traps {
        trap_data.extend_from_slice(&trap.code_offset.to_le_bytes());
        trap_data.extend_from_slice(&(trap.trap_code as u32).to_le_bytes());
    }
    let traps_section = object.add_section(
        object.segment_name(StandardSegment::Data).to_vec(),
        WASMER_TRAPS_SECTION_NAME.to_vec(),
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

    #[cfg(feature = "unwind")]
    if let Some(fde) = fde
        && let Some(cie) = create_systemv_cie(target.triple().architecture)
    {
        let mut frametable = FrameTable::default();
        let cie_id = frametable.add_cie(cie);
        match fde {
            UnwindFrame::SystemV(fde) => {
                frametable.add_fde(cie_id, fde);
            }
        }
        let mut eh_frame = EhFrame(WriterRelocate::new(target.triple().endianness().ok()));
        frametable
            .write_eh_frame(&mut eh_frame)
            .map_err(|e| CompileError::Codegen(format!("failed to write .eh_frame: {e}")))?;
        let eh_frame = eh_frame.0.into_section();
        let section = object.add_section(
            object.segment_name(StandardSegment::Debug).to_vec(),
            b".eh_frame".to_vec(),
            SectionKind::Other,
        );
        object.append_section_data(section, eh_frame.bytes.as_slice(), 4);
        add_relocations(
            &mut object,
            section,
            &eh_frame.relocations,
            Some((index, symbol)),
        )?;
    }
    #[cfg(not(feature = "unwind"))]
    let _ = fde;

    save_object(object, build_directory, kind.object_filename())
}

pub(crate) fn emit_function_body(
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

pub(crate) fn emit_import_trampoline(
    target: &Target,
    build_directory: &Path,
    index: usize,
    section: &CustomSection,
) -> Result<PathBuf, CompileError> {
    let mut object = get_object_for_target(target.triple())
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
    let symbol = object.add_symbol(Symbol {
        name: format!("i{index}").into_bytes(),
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
    save_object(object, build_directory, format!("i{index}.o"))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn link_module(
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
    module_file.read_to_end(&mut elf).map_err(|e| {
        CompileError::Codegen(format!("cannot read linked Singlepass artifact: {e}"))
    })?;
    Ok(Compilation::Elf(elf))
}
