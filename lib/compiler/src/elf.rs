//! Helpers shared by the object-based compiler backends (Cranelift and
//! Singlepass) for emitting per-function relocatable ELF objects and linking
//! them into the final module image.

use crate::compiler::{CompiledObjects, emit_metadata_and_link};
use crate::dwarf::{EhRelocation, EhTarget};
use crate::get_object_for_target;
use crate::misc::{CompiledFunctionExt, CompiledKind};
use crate::types::function::{Compilation, FunctionBody};
use object::{
    RelocationEncoding, RelocationFlags, RelocationKind as ObjectRelocationKind, SectionKind,
    SymbolFlags, SymbolKind, SymbolScope,
    write::{
        Object, Relocation as ObjectRelocation, StandardSection, StandardSegment, Symbol, SymbolId,
        SymbolSection,
    },
};
use std::path::PathBuf;
use wasmer_types::{CompileError, LibCall, TrapInformation, target::Target};

/// Serialized relocatable object and the unit's maximum stack usage, when known.
pub struct CompileOutput {
    /// Serialized relocatable object.
    pub object: Vec<u8>,
    /// The unit's maximum stack usage, when known.
    pub maximum_stack_usage: Option<usize>,
}

impl crate::compiler::CompiledFunction for CompileOutput {}

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
    let name = if object.format() == object::BinaryFormat::MachO {
        format!("_{}", libcall.to_function_name())
    } else {
        libcall.to_function_name().to_string()
    };
    object.add_symbol(Symbol {
        name: name.into_bytes(),
        value: 0,
        size: 0,
        kind: SymbolKind::Unknown,
        scope: SymbolScope::Dynamic,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    })
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

    // Add the personality pointer lazily. CIEs reference this slot indirectly
    // and PC-relative, while the slot itself has an absolute relocation to the
    // runtime function. This mirrors the `DW.ref.*` convention used by native
    // compilers and, unlike a GOT-relative data relocation, works across the
    // 64-bit ELF architectures supported by Wasmer.
    let mut personality_reference_symbol = None;
    for relocation in relocations {
        let symbol = match relocation.target {
            EhTarget::Function => function_symbol,
            EhTarget::Personality => {
                if let Some(symbol) = personality_reference_symbol {
                    symbol
                } else {
                    let personality_symbol = add_libcall_symbol(object, LibCall::EHPersonality);
                    let personality_section =
                        object.section_id(StandardSection::ReadOnlyDataWithRel);
                    let reference_symbol = object.add_symbol(Symbol {
                        name: b"DW.ref.wasmer_eh_personality".to_vec(),
                        value: 0,
                        size: 8,
                        kind: SymbolKind::Data,
                        scope: SymbolScope::Compilation,
                        weak: false,
                        section: SymbolSection::Undefined,
                        flags: SymbolFlags::None,
                    });
                    let reference_offset =
                        object.add_symbol_data(reference_symbol, personality_section, &[0; 8], 8);
                    object
                        .add_relocation(
                            personality_section,
                            ObjectRelocation {
                                offset: reference_offset,
                                flags: RelocationFlags::Generic {
                                    kind: ObjectRelocationKind::Absolute,
                                    encoding: RelocationEncoding::Generic,
                                    size: 64,
                                },
                                symbol: personality_symbol,
                                addend: 0,
                            },
                        )
                        .map_err(|e| {
                            CompileError::Codegen(format!(
                                "failed to add personality reference relocation: {e}"
                            ))
                        })?;
                    personality_reference_symbol = Some(reference_symbol);
                    reference_symbol
                }
            }
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
    kind: &CompiledKind,
    body: &FunctionBody,
) -> Result<Vec<u8>, CompileError> {
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
    object
        .write()
        .map_err(|e| CompileError::Codegen(format!("failed to serialize object: {e}")))
}

/// Serialize an import trampoline's custom section into its own relocatable object file.
pub fn emit_import_trampoline(
    target: &Target,
    kind: &CompiledKind,
    section_bytes: &[u8],
) -> Result<Vec<u8>, CompileError> {
    let mut object = get_object_for_target(target.triple())
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;
    let symbol = object.add_symbol(Symbol {
        name: kind.linkage_name().into_bytes(),
        value: 0,
        size: section_bytes.len() as u64,
        kind: SymbolKind::Text,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Undefined,
        flags: SymbolFlags::None,
    });
    let text = object.section_id(StandardSection::Text);
    object.add_symbol_data(symbol, text, section_bytes, 4);
    object
        .write()
        .map_err(|e| CompileError::Codegen(format!("failed to serialize object: {e}")))
}
/// Link all the per-function and trampoline objects (plus the Wasmer metadata
/// object) into the final shared-object module image.
#[allow(clippy::too_many_arguments)]
pub fn link_module(
    pool: &rayon::ThreadPool,
    target: &Target,
    compile_info_blob: &[u8],
    object_files: Vec<CompileOutput>,
    import_trampoline_objects: Vec<CompileOutput>,
    trampoline_objects: Vec<CompileOutput>,
    dynamic_trampoline_objects: Vec<CompileOutput>,
    debug_dir: Option<PathBuf>,
    module_hash: Option<String>,
) -> Result<Compilation, CompileError> {
    emit_metadata_and_link(
        pool,
        target,
        compile_info_blob,
        CompiledObjects {
            emit_trap_offsets: true,
            object_files,
            import_trampoline_object_files: import_trampoline_objects,
            trampoline_object_files: trampoline_objects,
            dynamic_trampoline_object_files: dynamic_trampoline_objects,
        },
        debug_dir,
        module_hash,
    )
}
