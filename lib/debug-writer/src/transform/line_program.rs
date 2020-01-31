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
// This file is from the WasmTime project.
// It was copied at revision `cc6e8e1af25e5f9b64e183970d50f62c8338f259`.
//
// Changes to this file are copyright of Wasmer inc. unless otherwise indicated
// and are licensed under the Wasmer project's license.
use super::address_transform::AddressTransform;
use super::attr::clone_attr_string;
use super::{Reader, TransformError};
use anyhow::Error;
use cranelift_entity::EntityRef;
use gimli::{
    write, DebugLine, DebugLineOffset, DebugStr, DebuggingInformationEntry, LineEncoding, Unit,
};
use std::collections::BTreeMap;
use std::iter::FromIterator;

#[derive(Debug)]
enum SavedLineProgramRow {
    Normal {
        address: u64,
        op_index: u64,
        file_index: u64,
        line: u64,
        column: u64,
        discriminator: u64,
        is_stmt: bool,
        basic_block: bool,
        prologue_end: bool,
        epilogue_begin: bool,
        isa: u64,
    },
    EndOfSequence(u64),
}

#[derive(Debug, Eq, PartialEq)]
enum ReadLineProgramState {
    SequenceEnded,
    ReadSequence,
    IgnoreSequence,
}

pub(crate) fn clone_line_program<R>(
    unit: &Unit<R, R::Offset>,
    root: &DebuggingInformationEntry<R>,
    addr_tr: &AddressTransform,
    out_encoding: gimli::Encoding,
    debug_str: &DebugStr<R>,
    debug_line: &DebugLine<R>,
    out_strings: &mut write::StringTable,
) -> Result<(write::LineProgram, DebugLineOffset, Vec<write::FileId>), Error>
where
    R: Reader,
{
    let offset = match root.attr_value(gimli::DW_AT_stmt_list)? {
        Some(gimli::AttributeValue::DebugLineRef(offset)) => offset,
        _ => {
            return Err(TransformError("Debug line offset is not found").into());
        }
    };
    let comp_dir = root.attr_value(gimli::DW_AT_comp_dir)?;
    let comp_name = root.attr_value(gimli::DW_AT_name)?;
    let out_comp_dir = clone_attr_string(
        comp_dir.as_ref().expect("comp_dir"),
        gimli::DW_FORM_strp,
        debug_str,
        out_strings,
    )?;
    let out_comp_name = clone_attr_string(
        comp_name.as_ref().expect("comp_name"),
        gimli::DW_FORM_strp,
        debug_str,
        out_strings,
    )?;

    let program = debug_line.program(
        offset,
        unit.header.address_size(),
        comp_dir.and_then(|val| val.string_value(&debug_str)),
        comp_name.and_then(|val| val.string_value(&debug_str)),
    );
    if let Ok(program) = program {
        let header = program.header();
        assert!(header.version() <= 4, "not supported 5");
        let line_encoding = LineEncoding {
            minimum_instruction_length: header.minimum_instruction_length(),
            maximum_operations_per_instruction: header.maximum_operations_per_instruction(),
            default_is_stmt: header.default_is_stmt(),
            line_base: header.line_base(),
            line_range: header.line_range(),
        };
        let mut out_program = write::LineProgram::new(
            out_encoding,
            line_encoding,
            out_comp_dir,
            out_comp_name,
            None,
        );
        let mut dirs = Vec::new();
        dirs.push(out_program.default_directory());
        for dir_attr in header.include_directories() {
            let dir_id = out_program.add_directory(clone_attr_string(
                dir_attr,
                gimli::DW_FORM_string,
                debug_str,
                out_strings,
            )?);
            dirs.push(dir_id);
        }
        let mut files = Vec::new();
        for file_entry in header.file_names() {
            let dir_id = dirs[file_entry.directory_index() as usize];
            let file_id = out_program.add_file(
                clone_attr_string(
                    &file_entry.path_name(),
                    gimli::DW_FORM_string,
                    debug_str,
                    out_strings,
                )?,
                dir_id,
                None,
            );
            files.push(file_id);
        }

        let mut rows = program.rows();
        let mut saved_rows = BTreeMap::new();
        let mut state = ReadLineProgramState::SequenceEnded;
        while let Some((_header, row)) = rows.next_row()? {
            if state == ReadLineProgramState::IgnoreSequence {
                if row.end_sequence() {
                    state = ReadLineProgramState::SequenceEnded;
                }
                continue;
            }
            let saved_row = if row.end_sequence() {
                state = ReadLineProgramState::SequenceEnded;
                SavedLineProgramRow::EndOfSequence(row.address())
            } else {
                if state == ReadLineProgramState::SequenceEnded {
                    // Discard sequences for non-existent code.
                    if row.address() == 0 {
                        state = ReadLineProgramState::IgnoreSequence;
                        continue;
                    }
                    state = ReadLineProgramState::ReadSequence;
                }
                SavedLineProgramRow::Normal {
                    address: row.address(),
                    op_index: row.op_index(),
                    file_index: row.file_index(),
                    line: row.line().unwrap_or(0),
                    column: match row.column() {
                        gimli::ColumnType::LeftEdge => 0,
                        gimli::ColumnType::Column(val) => val,
                    },
                    discriminator: row.discriminator(),
                    is_stmt: row.is_stmt(),
                    basic_block: row.basic_block(),
                    prologue_end: row.prologue_end(),
                    epilogue_begin: row.epilogue_begin(),
                    isa: row.isa(),
                }
            };
            saved_rows.insert(row.address(), saved_row);
        }

        let saved_rows = Vec::from_iter(saved_rows.into_iter());
        for (i, map) in addr_tr.map() {
            if map.len == 0 {
                continue; // no code generated
            }
            let symbol = i.index();
            let base_addr = map.offset;
            out_program.begin_sequence(Some(write::Address::Symbol { symbol, addend: 0 }));
            // TODO track and place function declaration line here
            let mut last_address = None;
            for addr_map in map.addresses.iter() {
                let saved_row = match saved_rows.binary_search_by_key(&addr_map.wasm, |i| i.0) {
                    Ok(i) => Some(&saved_rows[i].1),
                    Err(i) => {
                        if i > 0 {
                            Some(&saved_rows[i - 1].1)
                        } else {
                            None
                        }
                    }
                };
                if let Some(SavedLineProgramRow::Normal {
                    address,
                    op_index,
                    file_index,
                    line,
                    column,
                    discriminator,
                    is_stmt,
                    basic_block,
                    prologue_end,
                    epilogue_begin,
                    isa,
                }) = saved_row
                {
                    // Ignore duplicates
                    if Some(*address) != last_address {
                        let address_offset = if last_address.is_none() {
                            // Extend first entry to the function declaration
                            // TODO use the function declaration line instead
                            0
                        } else {
                            (addr_map.generated - base_addr) as u64
                        };
                        out_program.row().address_offset = address_offset;
                        out_program.row().op_index = *op_index;
                        out_program.row().file = files[(file_index - 1) as usize];
                        out_program.row().line = *line;
                        out_program.row().column = *column;
                        out_program.row().discriminator = *discriminator;
                        out_program.row().is_statement = *is_stmt;
                        out_program.row().basic_block = *basic_block;
                        out_program.row().prologue_end = *prologue_end;
                        out_program.row().epilogue_begin = *epilogue_begin;
                        out_program.row().isa = *isa;
                        out_program.generate_row();
                        last_address = Some(*address);
                    }
                }
            }
            let end_addr = (map.offset + map.len - 1) as u64;
            out_program.end_sequence(end_addr);
        }
        Ok((out_program, offset, files))
    } else {
        Err(TransformError("Valid line program not found").into())
    }
}
