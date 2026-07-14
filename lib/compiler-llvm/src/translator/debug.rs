use std::{collections::BTreeMap, path::Path, rc::Rc};

use addr2line::gimli::{Dwarf, EndianRcSlice, LittleEndian, SectionId};
use wasmer_compiler::{
    FunctionBodyData, ModuleTranslationState,
    wasmparser::{BinaryReader, FunctionBody},
};
use wasmer_types::{LocalFunctionIndex, ModuleInfo, entity::PrimaryMap};

/// An original source position associated with a Wasm operator.
#[derive(Clone, Debug)]
pub(super) struct SourceLocation {
    pub(super) file: String,
    pub(super) directory: String,
    pub(super) line: u32,
    pub(super) column: u32,
}

/// Source locations recovered from the input Wasm module's DWARF sections.
///
/// Wasm DWARF addresses are relative to the beginning of the code-section
/// payload, whereas `FunctionBodyData` offsets are relative to the complete
/// module. This map performs that translation once before parallel codegen.
#[derive(Default)]
pub(crate) struct WasmSourceMap {
    locations: BTreeMap<usize, SourceLocation>,
}

impl WasmSourceMap {
    pub(crate) fn new(
        module: &ModuleInfo,
        translation: &ModuleTranslationState,
        functions: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Self {
        let Some(code_base) = translation.code_section_offset() else {
            return Self::default();
        };

        type Reader = EndianRcSlice<LittleEndian>;
        let dwarf = match Dwarf::<Reader>::load(|id: SectionId| {
            let data = module.custom_sections(id.name()).next().unwrap_or_default();
            Ok::<_, addr2line::gimli::Error>(Reader::new(Rc::from(data), LittleEndian))
        }) {
            Ok(dwarf) => dwarf,
            Err(_) => return Self::default(),
        };
        let context = match addr2line::Context::from_dwarf(dwarf) {
            Ok(context) => context,
            Err(_) => return Self::default(),
        };

        let mut locations = BTreeMap::new();
        for (_, input) in functions.iter() {
            let body = FunctionBody::new(BinaryReader::new(input.data, input.module_offset));
            let Ok(mut operators) = body.get_operators_reader() else {
                continue;
            };
            while !operators.eof() {
                let offset = operators.original_position();
                if operators.read().is_err() {
                    break;
                }
                let Some(address) = offset.checked_sub(code_base).map(|value| value as u64) else {
                    continue;
                };
                let Ok(Some(location)) = context.find_location(address) else {
                    continue;
                };
                let (Some(file), Some(line)) = (location.file, location.line) else {
                    continue;
                };
                let path = Path::new(file);
                locations.insert(
                    offset,
                    SourceLocation {
                        file: path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(file)
                            .to_owned(),
                        directory: path
                            .parent()
                            .and_then(|directory| directory.to_str())
                            .unwrap_or("")
                            .to_owned(),
                        line,
                        column: location.column.unwrap_or(0),
                    },
                );
            }
        }

        Self { locations }
    }

    pub(super) fn get(&self, wasm_offset: usize) -> Option<&SourceLocation> {
        self.locations.get(&wasm_offset)
    }

    pub(super) fn first_in_function(&self, body: &FunctionBodyData<'_>) -> Option<&SourceLocation> {
        self.locations
            .range(body.module_offset..body.module_offset.saturating_add(body.data.len()))
            .next()
            .map(|(_, location)| location)
    }
}
