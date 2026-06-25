use gimli::{
    Encoding, Format, LineEncoding, RunTimeEndian, SectionId, constants,
    write::{
        Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString,
        Result as GimliResult, Sections, Writer,
    },
};
use object::{
    RelocationEncoding, RelocationFlags, RelocationKind as ObjectRelocationKind, SectionKind,
    write::{Object, Relocation as ObjectRelocation, StandardSegment, SymbolId},
};
use wasmer_types::{CompileError, SourceLoc, target::Endianness};

#[derive(Clone, Debug)]
pub struct WriterRelocate {
    pub relocs: Vec<DebugRelocation>,
    writer: EndianVec<RunTimeEndian>,
}

impl WriterRelocate {
    pub fn new() -> Self {
        WriterRelocate {
            relocs: Vec::new(),
            writer: EndianVec::new(RunTimeEndian::Little),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.writer.into_vec()
    }
}

impl Writer for WriterRelocate {
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
            Address::Symbol { addend, .. } => {
                let offset = self.len() as u64;
                if size != 8 {
                    return Err(gimli::write::Error::InvalidAddress);
                }
                self.relocs.push(DebugRelocation {
                    offset,
                    kind: ObjectRelocationKind::Absolute,
                    size,
                    target: DebugRelocationTarget::Function,
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
            Address::Symbol { addend, .. }
                if eh_pe == (constants::DW_EH_PE_pcrel | constants::DW_EH_PE_sdata4)
                    && size == 8 =>
            {
                let offset = self.len() as u64;
                self.relocs.push(DebugRelocation {
                    offset,
                    kind: ObjectRelocationKind::Relative,
                    size: 4,
                    target: DebugRelocationTarget::Function,
                    addend,
                });
                self.write_udata(0, 4)
            }
            Address::Symbol { .. } => Err(gimli::write::Error::InvalidAddress),
        }
    }

    fn write_offset(&mut self, _val: usize, _section: SectionId, _size: u8) -> GimliResult<()> {
        // TODO: a more proper error type?
        Err(gimli::write::Error::OffsetOutOfBounds)
    }

    fn write_offset_at(
        &mut self,
        _offset: usize,
        _val: usize,
        _section: SectionId,
        _size: u8,
    ) -> GimliResult<()> {
        // TODO: a more proper error type?
        Err(gimli::write::Error::OffsetOutOfBounds)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DebugRelocation {
    pub(crate) offset: u64,
    pub(crate) kind: ObjectRelocationKind,
    pub(crate) size: u8,
    pub(crate) target: DebugRelocationTarget,
    pub(crate) addend: i64,
}

#[derive(Clone, Debug)]
pub(crate) enum DebugRelocationTarget {
    Function,
    Section(SectionId),
}

#[derive(Clone, Debug)]
struct DebugWriter {
    relocs: Vec<DebugRelocation>,
    writer: EndianVec<RunTimeEndian>,
}

impl DebugWriter {
    fn new(_endianness: Option<Endianness>) -> Self {
        let endianness = RunTimeEndian::Little;
        Self {
            relocs: Vec::new(),
            writer: EndianVec::new(endianness),
        }
    }

    fn into_parts(self) -> (Vec<u8>, Vec<DebugRelocation>) {
        (self.writer.into_vec(), self.relocs)
    }
}

impl Writer for DebugWriter {
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
            Address::Symbol { addend, .. } => {
                let offset = self.len() as u64;
                self.relocs.push(DebugRelocation {
                    offset,
                    kind: ObjectRelocationKind::Absolute,
                    size,
                    target: DebugRelocationTarget::Function,
                    addend,
                });
                self.write_udata(0, size)
            }
        }
    }

    fn write_offset(&mut self, val: usize, section: SectionId, size: u8) -> GimliResult<()> {
        let offset = self.len() as u64;
        self.relocs.push(DebugRelocation {
            offset,
            kind: ObjectRelocationKind::Absolute,
            size,
            target: DebugRelocationTarget::Section(section),
            addend: val as i64,
        });
        self.write_udata(0, size)
    }

    fn write_offset_at(
        &mut self,
        offset: usize,
        val: usize,
        section: SectionId,
        size: u8,
    ) -> GimliResult<()> {
        self.relocs.push(DebugRelocation {
            offset: offset as u64,
            kind: ObjectRelocationKind::Absolute,
            size,
            target: DebugRelocationTarget::Section(section),
            addend: val as i64,
        });
        self.write_udata_at(offset, 0, size)
    }
}

/// DWARF debug info state built incrementally during codegen.
pub struct DwarfState {
    dwarf: DwarfUnit,
    file_id: gimli::write::FileId,
    subprogram: gimli::write::UnitEntryId,
}

/// Initialize DWARF debug info for a function.
/// Begins the line program sequence and sets up CU attributes.
pub fn init_dwarf_unit(
    function_name: &str,
    module_name: Option<&str>,
) -> Result<DwarfState, CompileError> {
    let encoding = Encoding {
        address_size: 8,
        format: Format::Dwarf32,
        version: 4,
    };
    let mut dwarf = DwarfUnit::new(encoding);
    let comp_dir = dwarf.strings.add(".");
    let file_name_str = module_name.unwrap_or("<module>");
    let file_name = dwarf.strings.add(file_name_str);
    dwarf.unit.line_program = LineProgram::new(
        encoding,
        LineEncoding::default(),
        LineString::String(b".".to_vec()),
        None,
        LineString::String(file_name_str.as_bytes().to_vec()),
        None,
    );
    let dir_id = dwarf.unit.line_program.default_directory();
    let file_id = dwarf.unit.line_program.add_file(
        LineString::String(file_name_str.as_bytes().to_vec()),
        dir_id,
        None,
    );

    let function_address = Address::Symbol {
        symbol: 0,
        addend: 0,
    };
    dwarf
        .unit
        .line_program
        .begin_sequence(Some(function_address));

    let root = dwarf.unit.root();
    let cu = dwarf.unit.get_mut(root);
    cu.set(
        gimli::DW_AT_producer,
        AttributeValue::String(b"Wasmer (Singlepass)".to_vec()),
    );
    cu.set(
        gimli::DW_AT_language,
        AttributeValue::Language(gimli::DW_LANG_C),
    );
    cu.set(gimli::DW_AT_name, AttributeValue::StringRef(file_name));
    cu.set(gimli::DW_AT_comp_dir, AttributeValue::StringRef(comp_dir));
    cu.set(
        gimli::DW_AT_low_pc,
        AttributeValue::Address(function_address),
    );

    let subprogram = dwarf.unit.add(root, gimli::DW_TAG_subprogram);
    let entry = dwarf.unit.get_mut(subprogram);
    entry.set(
        gimli::DW_AT_name,
        AttributeValue::String(function_name.as_bytes().to_vec()),
    );
    entry.set(
        gimli::DW_AT_decl_file,
        AttributeValue::FileIndex(Some(file_id)),
    );
    entry.set(
        gimli::DW_AT_low_pc,
        AttributeValue::Address(function_address),
    );

    Ok(DwarfState {
        dwarf,
        file_id,
        subprogram,
    })
}

impl DwarfState {
    /// Emit a line program row for an instruction at the given code offset.
    pub fn add_row(&mut self, code_offset: u64, srcloc: SourceLoc) {
        if srcloc.is_default() {
            return;
        }
        let row = self.dwarf.unit.line_program.row();
        row.address_offset = code_offset;
        row.file = self.file_id;
        row.line = (srcloc.bits() as u64).saturating_add(1);
        row.column = 0;
        self.dwarf.unit.line_program.generate_row();
    }

    /// Finalize DWARF sections and write them into the object.
    pub fn write_sections(
        &mut self,
        object: &mut Object<'static>,
        function_symbol: SymbolId,
        _function_name: &str,
        body_len: u64,
        endianness: Option<Endianness>,
    ) -> Result<(), CompileError> {
        // End the line program sequence.
        self.dwarf.unit.line_program.end_sequence(body_len);

        // Set DW_AT_high_pc for CU and subprogram (body_len now known).
        let root = self.dwarf.unit.root();
        {
            let cu = self.dwarf.unit.get_mut(root);
            cu.set(gimli::DW_AT_high_pc, AttributeValue::Data8(body_len));
        }
        {
            let entry = self.dwarf.unit.get_mut(self.subprogram);
            entry.set(gimli::DW_AT_decl_line, AttributeValue::Udata(1));
            entry.set(gimli::DW_AT_high_pc, AttributeValue::Data8(body_len));
        }

        let mut sections = Sections::new(DebugWriter::new(endianness));
        self.dwarf
            .write(&mut sections)
            .map_err(|e| CompileError::Codegen(format!("failed to write DWARF debug info: {e}")))?;

        let mut object_sections = Vec::new();
        sections
            .for_each(|id, writer| {
                let (bytes, relocs) = writer.clone().into_parts();
                if bytes.is_empty() {
                    object_sections.push((id, None, relocs));
                } else {
                    let section = object.add_section(
                        object.segment_name(StandardSegment::Debug).to_vec(),
                        id.name().as_bytes().to_vec(),
                        SectionKind::Debug,
                    );
                    object.append_section_data(section, &bytes, 1);
                    object_sections.push((id, Some(section), relocs));
                }
                Ok::<_, gimli::write::Error>(())
            })
            .map_err(|e| CompileError::Codegen(format!("failed to collect DWARF sections: {e}")))?;

        for (_, section, relocs) in object_sections.clone() {
            let Some(section) = section else { continue };
            for reloc in relocs {
                let symbol = match reloc.target {
                    DebugRelocationTarget::Function => function_symbol,
                    DebugRelocationTarget::Section(target) => {
                        let Some((_, Some(target_section), _)) =
                            object_sections.iter().find(|(id, _, _)| *id == target)
                        else {
                            continue;
                        };
                        object.section_symbol(*target_section)
                    }
                };
                object
                    .add_relocation(
                        section,
                        ObjectRelocation {
                            offset: reloc.offset,
                            symbol,
                            addend: reloc.addend,
                            flags: RelocationFlags::Generic {
                                kind: ObjectRelocationKind::Absolute,
                                encoding: RelocationEncoding::Generic,
                                size: u8::checked_mul(reloc.size, 8).unwrap_or(64),
                            },
                        },
                    )
                    .map_err(|e| {
                        CompileError::Codegen(format!("failed to add DWARF relocation: {e}"))
                    })?;
            }
        }

        Ok(())
    }
}
