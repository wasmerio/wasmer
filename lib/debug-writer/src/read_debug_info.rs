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
// It was copied at revision `39e57e3e9ac9c15bef45eb77a2544a7c0b76501a`.
//
// Changes to this file are copyright of Wasmer inc. unless otherwise indicated
// and are licensed under the Wasmer project's license.

use gimli::{
    DebugAbbrev, DebugAddr, DebugInfo, DebugLine, DebugLineStr, DebugLoc, DebugLocLists,
    DebugRanges, DebugRngLists, DebugStr, DebugStrOffsets, DebugTypes, EndianSlice, LittleEndian,
    LocationLists, RangeLists,
};
use std::collections::HashMap;
use std::path::PathBuf;
use wasmparser::{self, ModuleReader, SectionCode};

trait Reader: gimli::Reader<Offset = usize, Endian = LittleEndian> {}

impl<'input> Reader for gimli::EndianSlice<'input, LittleEndian> {}

pub use wasmparser::Type as WasmType;

pub type Dwarf<'input> = gimli::Dwarf<gimli::EndianSlice<'input, LittleEndian>>;

#[derive(Debug)]
pub struct FunctionMetadata {
    pub params: Box<[WasmType]>,
    pub locals: Box<[(u32, WasmType)]>,
}

#[derive(Debug)]
pub struct WasmFileInfo {
    pub path: Option<PathBuf>,
    pub code_section_offset: u64,
    pub funcs: Box<[FunctionMetadata]>,
}

#[derive(Debug)]
pub struct NameSection {
    pub module_name: Option<String>,
    pub func_names: HashMap<u32, String>,
    pub locals_names: HashMap<u32, HashMap<u32, String>>,
}

#[derive(Debug)]
pub struct DebugInfoData<'a> {
    pub dwarf: Dwarf<'a>,
    pub name_section: Option<NameSection>,
    pub wasm_file: WasmFileInfo,
}

fn convert_sections<'a>(sections: HashMap<&str, &'a [u8]>) -> Dwarf<'a> {
    const EMPTY_SECTION: &[u8] = &[];

    let endian = LittleEndian;
    let debug_str = DebugStr::new(sections.get(".debug_str").unwrap_or(&EMPTY_SECTION), endian);
    let debug_abbrev = DebugAbbrev::new(
        sections.get(".debug_abbrev").unwrap_or(&EMPTY_SECTION),
        endian,
    );
    let debug_info = DebugInfo::new(
        sections.get(".debug_info").unwrap_or(&EMPTY_SECTION),
        endian,
    );
    let debug_line = DebugLine::new(
        sections.get(".debug_line").unwrap_or(&EMPTY_SECTION),
        endian,
    );

    if sections.contains_key(".debug_addr") {
        panic!("Unexpected .debug_addr");
    }

    let debug_addr = DebugAddr::from(EndianSlice::new(EMPTY_SECTION, endian));

    if sections.contains_key(".debug_line_str") {
        panic!("Unexpected .debug_line_str");
    }

    let debug_line_str = DebugLineStr::from(EndianSlice::new(EMPTY_SECTION, endian));
    let debug_str_sup = DebugStr::from(EndianSlice::new(EMPTY_SECTION, endian));

    if sections.contains_key(".debug_rnglists") {
        panic!("Unexpected .debug_rnglists");
    }

    let debug_ranges = match sections.get(".debug_ranges") {
        Some(section) => DebugRanges::new(section, endian),
        None => DebugRanges::new(EMPTY_SECTION, endian),
    };
    let debug_rnglists = DebugRngLists::new(EMPTY_SECTION, endian);
    let ranges = RangeLists::new(debug_ranges, debug_rnglists);

    if sections.contains_key(".debug_loclists") {
        panic!("Unexpected .debug_loclists");
    }

    let debug_loc = match sections.get(".debug_loc") {
        Some(section) => DebugLoc::new(section, endian),
        None => DebugLoc::new(EMPTY_SECTION, endian),
    };
    let debug_loclists = DebugLocLists::new(EMPTY_SECTION, endian);
    let locations = LocationLists::new(debug_loc, debug_loclists);

    if sections.contains_key(".debug_str_offsets") {
        panic!("Unexpected .debug_str_offsets");
    }

    let debug_str_offsets = DebugStrOffsets::from(EndianSlice::new(EMPTY_SECTION, endian));

    if sections.contains_key(".debug_types") {
        panic!("Unexpected .debug_types");
    }

    let debug_types = DebugTypes::from(EndianSlice::new(EMPTY_SECTION, endian));

    Dwarf {
        debug_abbrev,
        debug_addr,
        debug_info,
        debug_line,
        debug_line_str,
        debug_str,
        debug_str_offsets,
        debug_str_sup,
        debug_types,
        locations,
        ranges,
    }
}

fn read_name_section(reader: wasmparser::NameSectionReader) -> wasmparser::Result<NameSection> {
    let mut module_name = None;
    let mut func_names = HashMap::new();
    let mut locals_names = HashMap::new();
    for i in reader.into_iter() {
        match i? {
            wasmparser::Name::Module(m) => {
                module_name = Some(String::from(m.get_name()?));
            }
            wasmparser::Name::Function(f) => {
                let mut reader = f.get_map()?;
                while let Ok(naming) = reader.read() {
                    func_names.insert(naming.index, String::from(naming.name));
                }
            }
            wasmparser::Name::Local(l) => {
                let mut reader = l.get_function_local_reader()?;
                while let Ok(f) = reader.read() {
                    let mut names = HashMap::new();
                    let mut reader = f.get_map()?;
                    while let Ok(naming) = reader.read() {
                        names.insert(naming.index, String::from(naming.name));
                    }
                    locals_names.insert(f.func_index, names);
                }
            }
        }
    }
    let result = NameSection {
        module_name,
        func_names,
        locals_names,
    };
    Ok(result)
}

pub fn read_debug_info(data: &[u8]) -> DebugInfoData {
    let mut reader = ModuleReader::new(data).expect("reader");
    let mut sections = HashMap::new();
    let mut name_section = None;
    let mut code_section_offset = 0;

    let mut signatures_params: Vec<Box<[WasmType]>> = Vec::new();
    let mut func_params_refs: Vec<usize> = Vec::new();
    let mut func_locals: Vec<Box<[(u32, WasmType)]>> = Vec::new();

    while !reader.eof() {
        let section = reader.read().expect("section");
        match section.code {
            SectionCode::Custom { name, .. } => {
                if name.starts_with(".debug_") {
                    let mut reader = section.get_binary_reader();
                    let len = reader.bytes_remaining();
                    sections.insert(name, reader.read_bytes(len).expect("bytes"));
                }
                if name == "name" {
                    if let Ok(reader) = section.get_name_section_reader() {
                        if let Ok(section) = read_name_section(reader) {
                            name_section = Some(section);
                        }
                    }
                }
            }
            SectionCode::Type => {
                signatures_params = section
                    .get_type_section_reader()
                    .expect("type section")
                    .into_iter()
                    .map(|ft| ft.expect("type").params)
                    .collect::<Vec<_>>();
            }
            SectionCode::Function => {
                func_params_refs = section
                    .get_function_section_reader()
                    .expect("function section")
                    .into_iter()
                    .map(|index| index.expect("func index") as usize)
                    .collect::<Vec<_>>();
            }
            SectionCode::Code => {
                code_section_offset = section.range().start as u64;
                func_locals = section
                    .get_code_section_reader()
                    .expect("code section")
                    .into_iter()
                    .map(|body| {
                        let locals = body
                            .expect("body")
                            .get_locals_reader()
                            .expect("locals reader");
                        locals
                            .into_iter()
                            .collect::<Result<Vec<_>, _>>()
                            .expect("locals data")
                            .into_boxed_slice()
                    })
                    .collect::<Vec<_>>();
            }
            _ => (),
        }
    }

    let func_meta = func_params_refs
        .into_iter()
        .zip(func_locals.into_iter())
        .map(|(params_index, locals)| FunctionMetadata {
            params: signatures_params[params_index].clone(),
            locals,
        })
        .collect::<Vec<_>>();

    DebugInfoData {
        dwarf: convert_sections(sections),
        name_section,
        wasm_file: WasmFileInfo {
            path: None,
            code_section_offset,
            funcs: func_meta.into_boxed_slice(),
        },
    }
}
