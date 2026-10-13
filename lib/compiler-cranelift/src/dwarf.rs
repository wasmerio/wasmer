use cranelift_codegen::gimli::write::{Address, EndianVec, Result, Writer};
use cranelift_codegen::gimli::{RunTimeEndian, SectionId, constants};
use std::collections::HashMap;
use wasmer_compiler::types::relocation::{Relocation, RelocationKind, RelocationTarget};
use wasmer_types::{LibCall, LocalFunctionIndex, entity::EntityRef};

#[derive(Clone, Debug)]
pub struct WriterRelocate {
    pub relocs: Vec<Relocation>,
    writer: EndianVec<RunTimeEndian>,
    lsda_symbols: HashMap<usize, (RelocationTarget, u32)>,
}

impl WriterRelocate {
    pub const FUNCTION_SYMBOL: usize = 0;
    pub const PERSONALITY_SYMBOL: usize = 1;
}

impl Writer for WriterRelocate {
    type Endian = RunTimeEndian;

    fn endian(&self) -> Self::Endian {
        self.writer.endian()
    }

    fn len(&self) -> usize {
        self.writer.len()
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write(bytes)
    }

    fn write_at(&mut self, offset: usize, bytes: &[u8]) -> Result<()> {
        self.writer.write_at(offset, bytes)
    }

    fn write_address(&mut self, address: Address, size: u8) -> Result<()> {
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
                    self.write_udata(addend as _, size)
                } else if symbol == Self::PERSONALITY_SYMBOL {
                    let offset = self.len() as u32;
                    let kind = match size {
                        4 => RelocationKind::Abs4,
                        8 => RelocationKind::Abs8,
                        other => unimplemented!(
                            "dwarf relocation size for personality not supported: {}",
                            other
                        ),
                    };
                    self.relocs.push(Relocation {
                        kind,
                        reloc_target: RelocationTarget::LibCall(LibCall::EHPersonality),
                        offset,
                        addend,
                    });
                    self.write_udata(0, size)
                } else if let Some((target, base)) = self.lsda_symbols.get(&symbol) {
                    let offset = self.len() as u32;
                    let kind = match size {
                        4 => RelocationKind::Abs4,
                        8 => RelocationKind::Abs8,
                        other => unimplemented!(
                            "dwarf relocation size for LSDA not supported: {}",
                            other
                        ),
                    };
                    self.relocs.push(Relocation {
                        kind,
                        reloc_target: *target,
                        offset,
                        addend: *base as i64 + addend,
                    });
                    self.write_udata(0, size)
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
    ) -> Result<()> {
        if eh_pe == constants::DW_EH_PE_absptr {
            self.write_address(address, size)
        } else {
            match address {
                Address::Constant(_) => self.writer.write_eh_pointer(address, eh_pe, size),
                Address::Symbol { .. } => {
                    unimplemented!("eh pointer encoding {eh_pe:?} not supported for symbol targets")
                }
            }
        }
    }

    fn write_offset(&mut self, _val: usize, _section: SectionId, _size: u8) -> Result<()> {
        unimplemented!("write_offset not yet implemented");
    }

    fn write_offset_at(
        &mut self,
        _offset: usize,
        _val: usize,
        _section: SectionId,
        _size: u8,
    ) -> Result<()> {
        unimplemented!("write_offset_at not yet implemented");
    }
}
