//   Copyright 2019 WasmTime Project Developers
//
//   Licensed under the Apache License, Version 2.0 (the "License");
//   you may not use this file except in compliance with the License.
//   You may obtain a copy of the License at
//
//       http://www.apache.org/licenses/LICENSE-2.0
//
//   Unless required by applicable law or agreed to in writing, software
//   distributed under the License is distributed on an "AS IS" BASIS,
//   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//   See the License for the specific language governing permissions and
//   limitations under the License.
//
// This file is from the WasmTime project. It reads DWARF info from a Wasm module.
// It was copied at revision `ea73d4fa91399349fd8d42e833363a0b1cad9f1c`.
//
// Changes to this file are copyright of Wasmer inc. unless otherwise indicated
// and are licensed under the Wasmer project's license.

use faerie::artifact::{Decl, SectionKind};
use faerie::*;
use gimli::write::{Address, Dwarf, EndianVec, Result, Sections, Writer};
use gimli::{RunTimeEndian, SectionId};

#[derive(Clone)]
struct DebugReloc {
    offset: u32,
    size: u8,
    name: String,
    addend: i64,
}

pub enum ResolvedSymbol {
    PhysicalAddress(u64),
    Reloc { name: String, addend: i64 },
}

pub trait SymbolResolver {
    fn resolve_symbol(&self, symbol: usize, addend: i64) -> ResolvedSymbol;
}

pub fn emit_dwarf(
    artifact: &mut Artifact,
    mut dwarf: Dwarf,
    symbol_resolver: &dyn SymbolResolver,
) -> anyhow::Result<()> {
    let endian = RunTimeEndian::Little;

    let mut sections = Sections::new(WriterRelocate::new(endian, symbol_resolver));
    dwarf.write(&mut sections)?;
    sections.for_each_mut(|id, s| -> anyhow::Result<()> {
        artifact.declare_with(
            id.name(),
            Decl::section(SectionKind::Debug),
            s.writer.take(),
        )
    })?;
    sections.for_each_mut(|id, s| -> anyhow::Result<()> {
        for reloc in &s.relocs {
            artifact.link_with(
                faerie::Link {
                    from: id.name(),
                    to: &reloc.name,
                    at: u64::from(reloc.offset),
                },
                faerie::Reloc::Debug {
                    size: reloc.size,
                    addend: reloc.addend as i32,
                },
            )?;
        }
        Ok(())
    })?;
    Ok(())
}

#[derive(Clone)]
pub struct WriterRelocate<'a> {
    relocs: Vec<DebugReloc>,
    writer: EndianVec<RunTimeEndian>,
    symbol_resolver: &'a dyn SymbolResolver,
}

impl<'a> WriterRelocate<'a> {
    pub fn new(endian: RunTimeEndian, symbol_resolver: &'a dyn SymbolResolver) -> Self {
        WriterRelocate {
            relocs: Vec::new(),
            writer: EndianVec::new(endian),
            symbol_resolver,
        }
    }
}

impl<'a> Writer for WriterRelocate<'a> {
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
                match self.symbol_resolver.resolve_symbol(symbol, addend as i64) {
                    ResolvedSymbol::PhysicalAddress(addr) => self.write_udata(addr, size),
                    ResolvedSymbol::Reloc { name, addend } => {
                        let offset = self.len() as u64;
                        self.relocs.push(DebugReloc {
                            offset: offset as u32,
                            size,
                            name,
                            addend,
                        });
                        self.write_udata(addend as u64, size)
                    }
                }
            }
        }
    }

    fn write_offset(&mut self, val: usize, section: SectionId, size: u8) -> Result<()> {
        let offset = self.len() as u32;
        let name = section.name().to_string();
        self.relocs.push(DebugReloc {
            offset,
            size,
            name,
            addend: val as i64,
        });
        self.write_udata(val as u64, size)
    }

    fn write_offset_at(
        &mut self,
        offset: usize,
        val: usize,
        section: SectionId,
        size: u8,
    ) -> Result<()> {
        let name = section.name().to_string();
        self.relocs.push(DebugReloc {
            offset: offset as u32,
            size,
            name,
            addend: val as i64,
        });
        self.write_udata_at(offset, val as u64, size)
    }
}
