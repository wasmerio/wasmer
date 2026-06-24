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
use wasmer_compiler::types::{
    address_map::FunctionAddressMap,
    relocation::{Relocation, RelocationKind, RelocationTarget},
    section::{CustomSection, CustomSectionProtection, SectionBody},
};
use wasmer_types::{
    CompileError, LocalFunctionIndex, SourceLoc, entity::EntityRef, target::Endianness,
};

#[derive(Clone, Debug)]
pub struct WriterRelocate {
    pub relocs: Vec<Relocation>,
    writer: EndianVec<RunTimeEndian>,
}

impl WriterRelocate {
    pub const FUNCTION_SYMBOL: usize = 0;
    pub fn new(endianness: Option<Endianness>) -> Self {
        let endianness = match endianness {
            Some(Endianness::Little) => RunTimeEndian::Little,
            Some(Endianness::Big) => RunTimeEndian::Big,
            // We autodetect it, based on the host
            None => RunTimeEndian::default(),
        };
        WriterRelocate {
            relocs: Vec::new(),
            writer: EndianVec::new(endianness),
        }
    }

    pub fn into_section(mut self) -> CustomSection {
        // GCC expects a terminating "empty" length, so write a 0 length at the end of the table.
        self.writer.write_u32(0).unwrap();
        let data = self.writer.into_vec();
        CustomSection {
            protection: CustomSectionProtection::Read,
            alignment: None,
            bytes: SectionBody::new_with_vec(data),
            relocations: self.relocs,
        }
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
            Address::Symbol { symbol, addend } => {
                // Is a function relocation
                if symbol == Self::FUNCTION_SYMBOL {
                    // We use the addend to detect the function index
                    let function_index = LocalFunctionIndex::new(addend as _);
                    let reloc_target = RelocationTarget::LocalFunc(function_index);
                    let offset = self.len() as u32;
                    let kind = match size {
                        8 => RelocationKind::Abs8,
                        _ => unimplemented!("dwarf relocation size not yet supported: {}", size),
                    };
                    let addend = 0;
                    self.relocs.push(Relocation {
                        kind,
                        reloc_target,
                        offset,
                        addend,
                    });
                    self.write_udata(addend as u64, size)
                } else {
                    unreachable!("Symbol {} in DWARF not recognized", symbol);
                }
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
                if symbol == Self::FUNCTION_SYMBOL
                    && eh_pe == (constants::DW_EH_PE_pcrel | constants::DW_EH_PE_sdata4)
                    && size == 8 =>
            {
                let function_index = LocalFunctionIndex::new(addend as _);
                let reloc_target = RelocationTarget::LocalFunc(function_index);
                let offset = self.len() as u32;
                self.relocs.push(Relocation {
                    kind: RelocationKind::PCRel4,
                    reloc_target,
                    offset,
                    addend: 0,
                });
                self.write_udata(0, 4)
            }
            Address::Symbol { .. } => {
                unimplemented!("eh pointer encoding {eh_pe:?} not supported for symbol targets")
            }
        }
    }

    fn write_offset(&mut self, _val: usize, _section: SectionId, _size: u8) -> GimliResult<()> {
        unimplemented!("write_offset not yet implemented");
    }

    fn write_offset_at(
        &mut self,
        _offset: usize,
        _val: usize,
        _section: SectionId,
        _size: u8,
    ) -> GimliResult<()> {
        unimplemented!("write_offset_at not yet implemented");
    }
}

#[derive(Clone, Debug)]
struct DebugRelocation {
    offset: u64,
    size: u8,
    target: DebugRelocationTarget,
    addend: i64,
}

#[derive(Clone, Debug)]
enum DebugRelocationTarget {
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
            Address::Symbol { symbol, addend } => {
                assert_eq!(symbol, WriterRelocate::FUNCTION_SYMBOL);
                let offset = self.len() as u64;
                self.relocs.push(DebugRelocation {
                    offset,
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
    let file_name_str = module_name
        .filter(|name| !name.is_empty())
        .unwrap_or("wasm-module");
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
        symbol: WriterRelocate::FUNCTION_SYMBOL,
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
        AttributeValue::String(b"wasmer singlepass".to_vec()),
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

/// Legacy entry point — kept for compatibility.
#[allow(dead_code)]
pub fn emit_debug_info(
    object: &mut Object<'static>,
    function_symbol: SymbolId,
    function_name: &str,
    module_name: Option<&str>,
    address_map: &FunctionAddressMap,
    endianness: Option<Endianness>,
) -> Result<(), CompileError> {
    let mut state = init_dwarf_unit(function_name, module_name)?;
    for inst in &address_map.instructions {
        state.add_row(inst.code_offset as u64, inst.srcloc);
    }
    state.write_sections(
        object,
        function_symbol,
        function_name,
        address_map.body_len as u64,
        endianness,
    )
}
