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
use std::path::{Path, PathBuf};
use wasmer_compiler::dwarf::init_dwarf_unit;
#[cfg(feature = "unwind")]
use wasmer_compiler::dwarf::{EhRelocation, EhTarget, WriterRelocate};
#[cfg(feature = "unwind")]
use wasmer_compiler::elf::emit_eh_frame_section;
use wasmer_compiler::{
    elf::{add_relocations, emit_trap_section, save_object},
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
            // GOT-indirect, PC-relative reference (`R_X86_64_GOTPCREL`). Used
            // for the personality routine, which is an undefined symbol resolved
            // at load time: routing it through the GOT yields a dynamic
            // relocation the runtime loader can apply (a plain data relocation
            // against an undefined symbol would be dropped by the linker).
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
                    kind: ObjectRelocationKind::GotRelative,
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
    Some((object.section_symbol(section), offsets))
}

/// Serialize a single compiled function into its own relocatable object file.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_local_function(
    #[cfg(feature = "unwind")] isa: &dyn TargetIsa,
    target: &Target,
    build_directory: &Path,
    index: LocalFunctionIndex,
    function_name: &str,
    module_name: Option<&str>,
    function: &CompiledFunction,
    #[cfg(feature = "unwind")] fde: Option<FrameDescriptionEntry>,
    #[cfg(feature = "unwind")] lsda: Option<FunctionLsdaData>,
) -> Result<PathBuf, CompileError> {
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
    if let Ok(mut dwarf_state) = init_dwarf_unit(function_name, module_name, "Wasmer (Cranelift)") {
        for instruction in &function.frame_info.address_map.instructions {
            dwarf_state.add_row(instruction.code_offset as u64, instruction.srcloc);
        }
        dwarf_state.write_sections(
            &mut object,
            function_symbol,
            function.body.body.len() as u64,
            target.triple().endianness().ok(),
        )?;
    }

    emit_trap_section(&mut object, &kind, &function.frame_info.traps);

    // Emit the per-function `.eh_frame` unwind table (and, for functions that
    // catch exceptions, the matching `.gcc_except_table` LSDA).
    #[cfg(feature = "unwind")]
    if let Some(fde) = fde
        && let Some(mut cie) = isa.create_systemv_cie()
    {
        let pointer_bytes = isa.frontend_config().pointer_bytes();

        // Emit the LSDA into `.gcc_except_table`, plus a per-object tag section
        // holding the exception tag constants referenced by its type table.
        let lsda_section_symbol = if let Some(lsda) = &lsda {
            let tag_section_symbol = emit_eh_tag_section(&mut object, lsda);

            let gcc_section = object.add_section(
                object.segment_name(StandardSegment::Data).to_vec(),
                b".gcc_except_table".to_vec(),
                SectionKind::ReadOnlyData,
            );
            let lsda_offset =
                object.append_section_data(gcc_section, &lsda.bytes, u64::from(pointer_bytes));
            // The type-table slots use `DW_EH_PE_pcrel | sdata4` encoding,
            // so their relocations are PC-relative 32-bit (`R_X86_64_PC32`).
            // This keeps `.gcc_except_table` position-independent and read-only.
            let tag_relocation_flags = RelocationFlags::Generic {
                kind: object::RelocationKind::Relative,
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
                    .add_relocation(
                        gcc_section,
                        ObjectRelocation {
                            offset: lsda_offset + reloc.offset as u64,
                            flags: tag_relocation_flags,
                            symbol: tag_symbol,
                            addend: tag_offset as i64,
                        },
                    )
                    .map_err(|e| {
                        CompileError::Codegen(format!("failed to add LSDA relocation: {e}"))
                    })?;
            }
            Some(object.section_symbol(gcc_section))
        } else {
            None
        };

        // The ELF image may be mapped at any base address, so make the FDE's
        // function reference position-independent.
        cie.fde_address_encoding = constants::DW_EH_PE_pcrel | constants::DW_EH_PE_sdata4;
        let mut fde = fde;
        if lsda_section_symbol.is_some() {
            // The personality routine is an undefined symbol resolved at load
            // time. Reference it GOT-indirect (PC-relative) so the linker emits
            // a GOT slot with a dynamic relocation the runtime loader applies; a
            // plain data relocation against an undefined symbol would be
            // dropped. The LSDA lives in the same image and is referenced
            // directly, PC-relative.
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

    save_object(object, build_directory, kind.object_filename())
}
