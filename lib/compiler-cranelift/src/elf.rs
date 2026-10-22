//! Emission of per-function relocatable ELF objects for the experimental
//! ELF artifact format.

#[cfg(feature = "unwind")]
use crate::eh::FunctionLsdaData;
#[cfg(feature = "unwind")]
use cranelift_codegen::gimli::{
    RunTimeEndian, SectionId as GimliSectionId, constants,
    write::{
        Address, EhFrame, EndianVec, FrameDescriptionEntry, FrameTable, Result as GimliResult,
        Writer,
    },
};
#[cfg(feature = "unwind")]
use cranelift_codegen::isa::TargetIsa;
#[cfg(feature = "unwind")]
use object::RelocationKind as ObjectRelocationKind;
use object::{
    RelocationEncoding, RelocationFlags, SectionKind, SymbolFlags, SymbolKind, SymbolScope,
    write::{
        Object, Relocation as ObjectRelocation, StandardSection, StandardSegment, Symbol,
        SymbolSection,
    },
};
#[cfg(feature = "unwind")]
use std::collections::HashMap;
#[cfg(feature = "unwind")]
use wasmer_compiler::elf::emit_eh_frame_section;
#[cfg(feature = "unwind")]
use wasmer_compiler::{
    WASMER_EH_TAGS_SECTION_NAME,
    dwarf::{EhRelocation, EhTarget, WriterRelocate},
};
use wasmer_compiler::{WasmSourceMap, dwarf::init_dwarf_unit};
use wasmer_compiler::{
    elf::{add_relocations, emit_trap_section},
    misc::{CompiledFunctionExt, CompiledKind},
    object::get_object_for_target,
    types::function::CompiledFunction,
};
use wasmer_types::{CompileError, LocalFunctionIndex, target::Target};

/// A gimli [`Writer`] for the `.eh_frame` section that records the relocations
/// required against the function symbol, the personality routine and the LSDA.
///
/// This mirrors [`wasmer_compiler::dwarf::WriterRelocate`], but implements the
/// `Writer` trait of the gimli version re-exported by `cranelift-codegen`
/// (which differs from the workspace gimli version), so that Cranelift's
/// `FrameTable`/`FrameDescriptionEntry` types can be serialized with it. The
/// recorded relocations use the shared [`EhRelocation`] representation.
#[cfg(feature = "unwind")]
struct EhFrameWriter {
    relocs: Vec<EhRelocation>,
    writer: EndianVec<RunTimeEndian>,
}

#[cfg(feature = "unwind")]
impl EhFrameWriter {
    fn new() -> Self {
        Self {
            relocs: Vec::new(),
            writer: EndianVec::new(RunTimeEndian::Little),
        }
    }

    fn into_bytes(self) -> Vec<u8> {
        self.writer.into_vec()
    }

    fn target_for(symbol: usize) -> GimliResult<EhTarget> {
        match symbol {
            WriterRelocate::FUNCTION_SYMBOL => Ok(EhTarget::Function),
            WriterRelocate::PERSONALITY_SYMBOL => Ok(EhTarget::Personality),
            WriterRelocate::LSDA_SYMBOL => Ok(EhTarget::Lsda),
            _ => Err(cranelift_codegen::gimli::write::Error::InvalidAddress),
        }
    }
}

#[cfg(feature = "unwind")]
impl Writer for EhFrameWriter {
    type Endian = RunTimeEndian;

    fn endian(&self) -> Self::Endian {
        self.writer.endian()
    }

    fn len(&self) -> usize {
        self.writer.len()
    }

    fn write(&mut self, bytes: &[u8]) -> GimliResult<()> {
        self.writer.write(bytes)
    }

    fn write_at(&mut self, offset: usize, bytes: &[u8]) -> GimliResult<()> {
        self.writer.write_at(offset, bytes)
    }

    fn write_address(&mut self, address: Address, size: u8) -> GimliResult<()> {
        match address {
            Address::Constant(val) => self.write_udata(val, size),
            Address::Symbol { symbol, addend } => {
                let target = Self::target_for(symbol)?;
                let offset = self.len() as u64;
                self.relocs.push(EhRelocation {
                    offset,
                    kind: ObjectRelocationKind::Absolute,
                    size,
                    target,
                    addend,
                });
                self.write_udata(0, size)
            }
        }
    }

    fn write_eh_pointer(
        &mut self,
        address: Address,
        eh_pe: constants::DwEhPe,
        size: u8,
    ) -> GimliResult<()> {
        if eh_pe == constants::DW_EH_PE_absptr {
            return self.write_address(address, size);
        }

        match address {
            Address::Constant(_) => self.writer.write_eh_pointer(address, eh_pe, size),
            Address::Symbol { symbol, addend }
                if eh_pe == (constants::DW_EH_PE_pcrel | constants::DW_EH_PE_sdata4)
                    && size == 8 =>
            {
                let target = Self::target_for(symbol)?;
                let offset = self.len() as u64;
                self.relocs.push(EhRelocation {
                    offset,
                    kind: ObjectRelocationKind::Relative,
                    size: 4,
                    target,
                    addend,
                });
                self.write_udata(0, 4)
            }
            // Indirect, PC-relative reference to the personality pointer. The
            // ELF emitter places the pointer in relocatable read-only data and
            // resolves this relocation against that local slot. This is the
            // architecture-independent equivalent of a `DW.ref.*` symbol.
            Address::Symbol { symbol, addend }
                if eh_pe
                    == (constants::DW_EH_PE_indirect
                        | constants::DW_EH_PE_pcrel
                        | constants::DW_EH_PE_sdata4)
                    && size == 8 =>
            {
                let target = Self::target_for(symbol)?;
                let offset = self.len() as u64;
                self.relocs.push(EhRelocation {
                    offset,
                    kind: ObjectRelocationKind::Relative,
                    size: 4,
                    target,
                    addend,
                });
                self.write_udata(0, 4)
            }
            Address::Symbol { .. } => Err(cranelift_codegen::gimli::write::Error::InvalidAddress),
        }
    }

    fn write_offset(
        &mut self,
        _val: usize,
        _section: GimliSectionId,
        _size: u8,
    ) -> GimliResult<()> {
        Err(cranelift_codegen::gimli::write::Error::OffsetOutOfBounds)
    }

    fn write_offset_at(
        &mut self,
        _offset: usize,
        _val: usize,
        _section: GimliSectionId,
        _size: u8,
    ) -> GimliResult<()> {
        Err(cranelift_codegen::gimli::write::Error::OffsetOutOfBounds)
    }
}

/// Emit a per-object section holding the exception tag constants referenced by
/// a function's LSDA type table, returning a section symbol and a tag->offset
/// map. Returns `None` when the LSDA references no tags.
#[cfg(feature = "unwind")]
fn emit_eh_tag_section(
    object: &mut Object<'static>,
    lsda: &FunctionLsdaData,
) -> Option<(object::write::SymbolId, HashMap<u32, u32>)> {
    let mut tags: Vec<u32> = lsda.relocations.iter().map(|r| r.tag).collect();
    tags.sort_unstable();
    tags.dedup();
    if tags.is_empty() {
        return None;
    }

    let mut bytes = Vec::with_capacity(tags.len() * size_of::<u32>());
    let mut offsets = HashMap::new();
    for tag in tags {
        offsets.insert(tag, bytes.len() as u32);
        bytes.extend_from_slice(&tag.to_ne_bytes());
    }

    let section = object.add_section(
        object.segment_name(StandardSegment::Data).to_vec(),
        b".wasmer.eh_tags".to_vec(),
        SectionKind::ReadOnlyData,
    );
    object.append_section_data(section, &bytes, 4);
    let symbol = if object.format() == object::BinaryFormat::MachO {
        object.add_symbol(Symbol {
            name: WASMER_EH_TAGS_SECTION_NAME.to_vec(),
            value: 0,
            size: bytes.len() as u64,
            kind: SymbolKind::Data,
            scope: SymbolScope::Compilation,
            weak: false,
            section: SymbolSection::Section(section),
            flags: SymbolFlags::None,
        })
    } else {
        object.section_symbol(section)
    };
    Some((symbol, offsets))
}

/// Serialize a single compiled function into its own relocatable object file.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_local_function(
    #[cfg(feature = "unwind")] isa: &dyn TargetIsa,
    target: &Target,
    index: LocalFunctionIndex,
    function_name: &str,
    module_name: Option<&str>,
    function: &CompiledFunction,
    source_map: &WasmSourceMap,
    #[cfg(feature = "unwind")] fde: Option<FrameDescriptionEntry>,
    #[cfg(feature = "unwind")] lsda: Option<FunctionLsdaData>,
    #[cfg(feature = "unwind")] compact_unwind_encoding: Option<u32>,
) -> Result<Vec<u8>, CompileError> {
    let kind = CompiledKind::Local(index, String::new());
    let mut object = get_object_for_target(target.triple())
        .map_err(|e| CompileError::Codegen(format!("cannot create object: {e}")))?;

    // Emit the function body into the text section.
    let function_symbol = object.add_symbol(Symbol {
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
    object.add_symbol_data(function_symbol, text, &function.body.body, 16);
    add_relocations(
        &mut object,
        text,
        &function.relocations,
        Some((index, function_symbol)),
    )?;

    // Populate DWARF line info from the address map.
    // TODO: support dSYM debug info file creation and loading in the future
    if matches!(
        target.triple().binary_format,
        target_lexicon::BinaryFormat::Elf
    ) && let Ok(mut dwarf_state) =
        init_dwarf_unit(function_name, module_name, "Wasmer (Cranelift)")
    {
        for instruction in &function.frame_info.address_map.instructions {
            dwarf_state.add_source_map_row(
                instruction.code_offset as u64,
                instruction.srcloc,
                source_map,
            );
        }
        dwarf_state.write_sections(
            &mut object,
            function_symbol,
            function.body.body.len() as u64,
            target.triple().endianness().ok(),
        )?;
    }

    emit_trap_section(&mut object, &kind, &function.frame_info.traps);

    #[cfg(feature = "unwind")]
    {
        let pointer_bytes = isa.frontend_config().pointer_bytes();
        let macho_compact_unwind = compact_unwind_encoding.is_some();

        // Emit the LSDA into `.gcc_except_table`, plus a per-object tag section
        // holding the exception tag constants referenced by its type table.
        let lsda_section_symbol = if let Some(lsda) = &lsda {
            let tag_section_symbol = emit_eh_tag_section(&mut object, lsda);

            let gcc_section = object.add_section(
                object.segment_name(StandardSegment::Data).to_vec(),
                if macho_compact_unwind {
                    b"__gcc_except_tab".to_vec()
                } else {
                    b".gcc_except_table".to_vec()
                },
                SectionKind::ReadOnlyData,
            );
            let lsda_offset =
                object.append_section_data(gcc_section, &lsda.bytes, u64::from(pointer_bytes));
            let subtractor = macho_compact_unwind.then(|| {
                use wasmer_compiler::WASMER_LSDA_SECTION_NAME;

                object.add_symbol(Symbol {
                    name: WASMER_LSDA_SECTION_NAME.to_vec(),
                    value: lsda_offset,
                    size: lsda.bytes.len() as u64,
                    kind: SymbolKind::Data,
                    scope: SymbolScope::Compilation,
                    weak: false,
                    section: SymbolSection::Section(gcc_section),
                    flags: SymbolFlags::None,
                })
            });
            // Mach-O represents PC-relative data as a symbol difference.
            let tag_relocation_flags = RelocationFlags::Generic {
                kind: if macho_compact_unwind {
                    object::RelocationKind::Absolute
                } else {
                    object::RelocationKind::Relative
                },
                encoding: RelocationEncoding::Generic,
                size: 32,
            };
            for reloc in &lsda.relocations {
                let (tag_symbol, tag_offset) = tag_section_symbol
                    .as_ref()
                    .and_then(|(symbol, offsets)| {
                        offsets.get(&reloc.tag).map(|offset| (*symbol, *offset))
                    })
                    .ok_or_else(|| {
                        CompileError::Codegen(format!(
                            "missing exception tag {} for LSDA relocation",
                            reloc.tag
                        ))
                    })?;
                object
                    .add_relocation_with_subtractor(
                        gcc_section,
                        ObjectRelocation {
                            offset: lsda_offset + reloc.offset as u64,
                            flags: tag_relocation_flags,
                            symbol: tag_symbol,
                            addend: tag_offset as i64
                                - if macho_compact_unwind {
                                    reloc.offset as i64
                                } else {
                                    0
                                },
                        },
                        subtractor,
                    )
                    .map_err(|e| {
                        CompileError::Codegen(format!("failed to add LSDA relocation: {e}"))
                    })?;
            }
            Some(object.section_symbol(gcc_section))
        } else {
            None
        };

        if let Some(encoding) = compact_unwind_encoding {
            let function_length = function.body.body.len().try_into().map_err(|_| {
                CompileError::Codegen("function body too large for Mach-O compact unwind".into())
            })?;
            let compact = crate::eh::build_compact_unwind_section(
                [crate::eh::CompactUnwindEntryData {
                    function: index,
                    function_length,
                    compact_encoding: encoding
                        | if lsda_section_symbol.is_some() {
                            0x4000_0000
                        } else {
                            0
                        },
                    lsda_offset: lsda_section_symbol.map(|_| 0),
                }],
                lsda_section_symbol
                    .map(|_| wasmer_compiler::types::section::SectionIndex::from_u32(0)),
            )
            .expect("one compact unwind entry");
            let section = object.add_section(
                object.segment_name(StandardSegment::Data).to_vec(),
                b"__compact_unwind".to_vec(),
                SectionKind::ReadOnlyData,
            );
            object.append_section_data(section, compact.bytes.as_slice(), 8);
            for relocation in &compact.relocations {
                let symbol = match relocation.reloc_target {
                    wasmer_compiler::types::relocation::RelocationTarget::LocalFunc(_) => {
                        object.section_symbol(text)
                    }
                    wasmer_compiler::types::relocation::RelocationTarget::LibCall(libcall) => {
                        wasmer_compiler::elf::add_libcall_symbol(&mut object, libcall)
                    }
                    wasmer_compiler::types::relocation::RelocationTarget::CustomSection(_) => {
                        lsda_section_symbol.expect("compact unwind LSDA")
                    }
                    _ => unreachable!("unexpected compact unwind relocation"),
                };
                object
                    .add_relocation(
                        section,
                        ObjectRelocation {
                            offset: relocation.offset.into(),
                            flags: wasmer_compiler::elf::relocation_kind_to_flags(relocation.kind)?,
                            symbol,
                            addend: relocation.addend,
                        },
                    )
                    .map_err(|e| {
                        CompileError::Codegen(format!(
                            "failed to add compact unwind relocation: {e}"
                        ))
                    })?;
            }
        }

        if let Some(fde) = fde
            && let Some(mut cie) = isa.create_systemv_cie()
        {
            // The ELF image may be mapped at any base address, so make the FDE's
            // function reference position-independent.
            cie.fde_address_encoding = constants::DW_EH_PE_pcrel | constants::DW_EH_PE_sdata4;
            let mut fde = fde;
            if lsda_section_symbol.is_some() {
                // Reference the personality pointer indirectly and PC-relative.
                // The shared ELF emitter creates the local pointer slot and its
                // dynamic relocation. The LSDA lives in the same image and is
                // referenced directly, PC-relative.
                cie.personality = Some((
                    constants::DW_EH_PE_indirect
                        | constants::DW_EH_PE_pcrel
                        | constants::DW_EH_PE_sdata4,
                    Address::Symbol {
                        symbol: WriterRelocate::PERSONALITY_SYMBOL,
                        addend: 0,
                    },
                ));
                cie.lsda_encoding = Some(constants::DW_EH_PE_pcrel | constants::DW_EH_PE_sdata4);
                fde.lsda = Some(Address::Symbol {
                    symbol: WriterRelocate::LSDA_SYMBOL,
                    addend: 0,
                });
            }

            let mut frametable = FrameTable::default();
            let cie_id = frametable.add_cie(cie);
            frametable.add_fde(cie_id, fde);

            let mut eh_frame = EhFrame(EhFrameWriter::new());
            frametable
                .write_eh_frame(&mut eh_frame)
                .map_err(|e| CompileError::Codegen(format!("failed to write .eh_frame: {e}")))?;

            let relocations = std::mem::take(&mut eh_frame.0.relocs);
            emit_eh_frame_section(
                &mut object,
                &eh_frame.0.into_bytes(),
                &relocations,
                function_symbol,
                lsda_section_symbol,
            )?;
        }
    }

    let mut bytes = object
        .write()
        .map_err(|e| CompileError::Codegen(format!("failed to serialize object: {e}")))?;
    if object.format() == object::BinaryFormat::MachO {
        use object::{Object as _, ObjectSection as _};

        // Mach-O local absolute relocations store an object address, not a
        // section-relative addend. The object writer leaves that adjustment
        // to its caller; the linker subtracts the target section's address.
        let file = object::File::parse(bytes.as_slice())
            .map_err(|e| CompileError::Codegen(format!("failed to read emitted object: {e}")))?;
        let mut adjustments = Vec::new();
        for section in file.sections() {
            let Some((file_offset, _)) = section.file_range() else {
                continue;
            };
            for (offset, relocation) in section.relocations() {
                if relocation.kind() == object::RelocationKind::Absolute
                    && relocation.size() == 64
                    && let object::RelocationTarget::Section(target) = relocation.target()
                {
                    let address = file
                        .section_by_index(target)
                        .map_err(|e| {
                            CompileError::Codegen(format!("invalid relocation section: {e}"))
                        })?
                        .address();
                    adjustments.push(((file_offset + offset) as usize, address));
                }
            }
        }
        for (offset, address) in adjustments {
            let value = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            bytes[offset..offset + 8].copy_from_slice(&value.wrapping_add(address).to_le_bytes());
        }
    }
    Ok(bytes)
}
