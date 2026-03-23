// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Translation skeleton that traverses the whole WebAssembly module and call helper functions
//! to deal with each part of it.
use super::environ::ModuleEnvironment;
use super::error::from_binaryreadererror_wasmerror;
use super::middleware::{MiddlewareBinaryReader, ModuleMiddleware, ModuleMiddlewareChain};
use super::sections::{
    parse_data_section, parse_element_section, parse_export_section, parse_function_section,
    parse_global_section, parse_import_section, parse_memory_section, parse_name_section,
    parse_start_section, parse_table_section, parse_tag_section, parse_type_section,
};
use super::state::ModuleTranslationState;
use itertools::Itertools;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use wasmer_types::entity::PrimaryMap;
use wasmer_types::{LocalFunctionIndex, ModuleInfo, TableIndex, WasmError, WasmResult};
use wasmparser::{BinaryReader, NameSectionReader, Parser, Payload};

use crate::translator::FunctionBinaryReader;

fn analyze_function_readonly_table(
    middlewares: &[Arc<dyn ModuleMiddleware>],
    local_func_index: LocalFunctionIndex,
    function_body: &super::environ::FunctionBodyData<'_>,
    table_index: TableIndex,
) -> WasmResult<bool> {
    let mut reader =
        MiddlewareBinaryReader::new_with_offset(function_body.data, function_body.module_offset);
    reader.set_middleware_chain(middlewares.generate_function_middleware_chain(local_func_index));

    let local_count = reader.read_local_count()?;
    for _ in 0..local_count {
        reader.read_local_decl()?;
    }

    while !reader.eof() {
        match reader.read_operator()? {
            wasmparser::Operator::TableCopy { dst_table, .. } => {
                if TableIndex::from_u32(dst_table) == table_index {
                    return Ok(false);
                }
            }
            wasmparser::Operator::TableInit { table, .. } => {
                if TableIndex::from_u32(table) == table_index {
                    return Ok(false);
                }
            }
            wasmparser::Operator::ElemDrop { .. } => return Ok(false),
            wasmparser::Operator::TableGrow { table } => {
                if TableIndex::from_u32(table) == table_index {
                    return Ok(false);
                }
            }
            _ => {}
        }
    }

    Ok(true)
}

fn analyze_readonly_funcref_table(
    module: &ModuleInfo,
    function_body_inputs: &PrimaryMap<LocalFunctionIndex, super::environ::FunctionBodyData<'_>>,
    middlewares: &[Arc<dyn ModuleMiddleware>],
) -> WasmResult<Option<TableIndex>> {
    let Ok(table_index) = module
        .tables
        .iter()
        .filter_map(|(index, table)| {
            if module.local_table_index(index).is_some() && table.is_fixed_funcref_table() {
                Some(index)
            } else {
                None
            }
        })
        .exactly_one()
    else {
        return Ok(None);
    };

    let start = Instant::now();

    let readonly = AtomicBool::new(true);
    function_body_inputs
        .iter()
        .collect_vec()
        .par_iter()
        .map(|(local_func_index, function_body)| {
            if !readonly.load(Ordering::Relaxed) {
                return Ok(());
            }

            if !analyze_function_readonly_table(
                middlewares,
                *local_func_index,
                function_body,
                table_index,
            )? {
                readonly.store(false, Ordering::Relaxed);
            }

            Ok(())
        })
        .collect::<WasmResult<Vec<_>>>()?;
    dbg!(Instant::now() - start);

    if !readonly.load(Ordering::Relaxed) {
        return Ok(None);
    }

    Ok(Some(table_index))
}

/// Translate a sequence of bytes forming a valid Wasm binary into a
/// parsed ModuleInfo `ModuleTranslationState`.
pub fn translate_module<'data>(
    data: &'data [u8],
    environ: &mut ModuleEnvironment<'data>,
) -> WasmResult<ModuleTranslationState> {
    let mut module_translation_state = ModuleTranslationState::new();

    for payload in Parser::new(0).parse_all(data) {
        match payload.map_err(from_binaryreadererror_wasmerror)? {
            Payload::Version { .. } | Payload::End { .. } => {}

            Payload::TypeSection(types) => {
                parse_type_section(types, &mut module_translation_state, environ)?;
            }

            Payload::ImportSection(imports) => {
                parse_import_section(imports, environ)?;
            }

            Payload::FunctionSection(functions) => {
                parse_function_section(functions, environ)?;
            }

            Payload::TableSection(tables) => {
                parse_table_section(tables, environ)?;
            }

            Payload::MemorySection(memories) => {
                parse_memory_section(memories, environ)?;
            }

            Payload::GlobalSection(globals) => {
                parse_global_section(globals, environ)?;
            }

            Payload::ExportSection(exports) => {
                parse_export_section(exports, environ)?;
            }

            Payload::StartSection { func, .. } => {
                parse_start_section(func, environ)?;
            }

            Payload::ElementSection(elements) => {
                parse_element_section(elements, environ)?;
            }

            Payload::CodeSectionStart { .. } => {}
            Payload::CodeSectionEntry(code) => {
                let mut code = code.get_binary_reader();
                let size = code.bytes_remaining();
                let offset = code.original_position();
                environ.define_function_body(
                    &module_translation_state,
                    code.read_bytes(size)
                        .map_err(from_binaryreadererror_wasmerror)?,
                    offset,
                )?;
            }

            Payload::DataSection(data) => {
                parse_data_section(data, environ)?;
            }

            Payload::DataCountSection { count, .. } => {
                environ.reserve_passive_data(count)?;
            }

            Payload::TagSection(t) => parse_tag_section(t, environ)?,

            Payload::CustomSection(sectionreader) => {
                // We still add the custom section data, but also read it as name section reader
                let name = sectionreader.name();
                environ.custom_section(name, sectionreader.data())?;
                if name == "name" {
                    parse_name_section(
                        NameSectionReader::new(BinaryReader::new(
                            sectionreader.data(),
                            sectionreader.data_offset(),
                        )),
                        environ,
                    )?;
                }
            }

            Payload::UnknownSection { .. } => unreachable!(),
            k => {
                return Err(WasmError::Unsupported(format!(
                    "Unsupported paylod kind: {k:?}"
                )));
            }
        }
    }

    environ
        .middlewares
        .apply_on_module_info(&mut environ.module)
        .map_err(WasmError::from)?;

    if let Some(table_index) = analyze_readonly_funcref_table(
        &environ.module,
        &environ.function_body_inputs,
        &environ.middlewares,
    )? {
        environ.module.tables[table_index].readonly = true;
    }

    Ok(module_translation_state)
}
