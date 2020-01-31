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
// It was copied at revision `907e7aac01af333a0af310ce0472abbc8a9adb6c`.
//
// Changes to this file are copyright of Wasmer inc. unless otherwise indicated
// and are licensed under the Wasmer project's license.
use super::address_transform::AddressTransform;
use super::expression::{compile_expression, CompiledExpression, FunctionFrameInfo};
use super::range_info_builder::RangeInfoBuilder;
use super::unit::PendingDieRef;
use super::{DebugInputContext, Reader, TransformError};
use anyhow::Error;
use gimli::{
    write, AttributeValue, DebugLineOffset, DebugStr, DebuggingInformationEntry, UnitOffset,
};
use std::collections::HashMap;

pub(crate) enum FileAttributeContext<'a> {
    Root(Option<DebugLineOffset>),
    Children(&'a Vec<write::FileId>, Option<&'a CompiledExpression>),
}

fn is_exprloc_to_loclist_allowed(attr_name: gimli::constants::DwAt) -> bool {
    match attr_name {
        gimli::DW_AT_location
        | gimli::DW_AT_string_length
        | gimli::DW_AT_return_addr
        | gimli::DW_AT_data_member_location
        | gimli::DW_AT_frame_base
        | gimli::DW_AT_segment
        | gimli::DW_AT_static_link
        | gimli::DW_AT_use_location
        | gimli::DW_AT_vtable_elem_location => true,
        _ => false,
    }
}

pub(crate) fn clone_die_attributes<'a, R>(
    entry: &DebuggingInformationEntry<R>,
    context: &DebugInputContext<R>,
    addr_tr: &'a AddressTransform,
    frame_info: Option<&FunctionFrameInfo>,
    unit_encoding: gimli::Encoding,
    out_unit: &mut write::Unit,
    current_scope_id: write::UnitEntryId,
    subprogram_range_builder: Option<RangeInfoBuilder>,
    scope_ranges: Option<&Vec<(u64, u64)>>,
    cu_low_pc: u64,
    out_strings: &mut write::StringTable,
    die_ref_map: &HashMap<UnitOffset, write::UnitEntryId>,
    pending_die_refs: &mut Vec<PendingDieRef>,
    file_context: FileAttributeContext<'a>,
) -> Result<(), Error>
where
    R: Reader,
{
    let _tag = &entry.tag();
    let endian = gimli::RunTimeEndian::Little;

    let range_info = if let Some(subprogram_range_builder) = subprogram_range_builder {
        subprogram_range_builder
    } else if entry.tag() == gimli::DW_TAG_compile_unit {
        // FIXME currently address_transform operate on a single func range,
        // once it is fixed we can properly set DW_AT_ranges attribute.
        // Using for now DW_AT_low_pc = 0.
        RangeInfoBuilder::Position(0)
    } else {
        RangeInfoBuilder::from(entry, context, unit_encoding, cu_low_pc)?
    };
    range_info.build(addr_tr, out_unit, current_scope_id);

    let mut attrs = entry.attrs();
    while let Some(attr) = attrs.next()? {
        let attr_value = match attr.value() {
            AttributeValue::Addr(_) if attr.name() == gimli::DW_AT_low_pc => {
                continue;
            }
            AttributeValue::Udata(_) if attr.name() == gimli::DW_AT_high_pc => {
                continue;
            }
            AttributeValue::RangeListsRef(_) if attr.name() == gimli::DW_AT_ranges => {
                continue;
            }
            AttributeValue::Exprloc(_) if attr.name() == gimli::DW_AT_frame_base => {
                continue;
            }

            AttributeValue::Addr(u) => {
                let addr = addr_tr.translate(u).unwrap_or(write::Address::Constant(0));
                write::AttributeValue::Address(addr)
            }
            AttributeValue::Udata(u) => write::AttributeValue::Udata(u),
            AttributeValue::Data1(d) => write::AttributeValue::Data1(d),
            AttributeValue::Data2(d) => write::AttributeValue::Data2(d),
            AttributeValue::Data4(d) => write::AttributeValue::Data4(d),
            AttributeValue::Sdata(d) => write::AttributeValue::Sdata(d),
            AttributeValue::Flag(f) => write::AttributeValue::Flag(f),
            AttributeValue::DebugLineRef(line_program_offset) => {
                if let FileAttributeContext::Root(o) = file_context {
                    if o != Some(line_program_offset) {
                        return Err(TransformError("invalid debug_line offset").into());
                    }
                    write::AttributeValue::LineProgramRef
                } else {
                    return Err(TransformError("unexpected debug_line index attribute").into());
                }
            }
            AttributeValue::FileIndex(i) => {
                if let FileAttributeContext::Children(file_map, _) = file_context {
                    write::AttributeValue::FileIndex(Some(file_map[(i - 1) as usize]))
                } else {
                    return Err(TransformError("unexpected file index attribute").into());
                }
            }
            AttributeValue::DebugStrRef(str_offset) => {
                let s = context.debug_str.get_str(str_offset)?.to_slice()?.to_vec();
                write::AttributeValue::StringRef(out_strings.add(s))
            }
            AttributeValue::RangeListsRef(r) => {
                let range_info =
                    RangeInfoBuilder::from_ranges_ref(r, context, unit_encoding, cu_low_pc)?;
                let range_list_id = range_info.build_ranges(addr_tr, &mut out_unit.ranges);
                write::AttributeValue::RangeListRef(range_list_id)
            }
            AttributeValue::LocationListsRef(r) => {
                let low_pc = 0;
                let mut locs = context.loclists.locations(
                    r,
                    unit_encoding,
                    low_pc,
                    &context.debug_addr,
                    context.debug_addr_base,
                )?;
                let frame_base = if let FileAttributeContext::Children(_, frame_base) = file_context
                {
                    frame_base
                } else {
                    None
                };
                let mut result = None;
                while let Some(loc) = locs.next()? {
                    if let Some(expr) = compile_expression(&loc.data, unit_encoding, frame_base)? {
                        if result.is_none() {
                            result = Some(Vec::new());
                        }
                        for (start, len, expr) in expr.build_with_locals(
                            &[(loc.range.begin, loc.range.end)],
                            addr_tr,
                            frame_info,
                            endian,
                        ) {
                            if len == 0 {
                                // Ignore empty range
                                continue;
                            }
                            result.as_mut().unwrap().push(write::Location::StartLength {
                                begin: start,
                                length: len,
                                data: expr,
                            });
                        }
                    } else {
                        // FIXME _expr contains invalid expression
                        continue; // ignore entry
                    }
                }
                if result.is_none() {
                    continue; // no valid locations
                }
                let list_id = out_unit.locations.add(write::LocationList(result.unwrap()));
                write::AttributeValue::LocationListRef(list_id)
            }
            AttributeValue::Exprloc(ref expr) => {
                let frame_base = if let FileAttributeContext::Children(_, frame_base) = file_context
                {
                    frame_base
                } else {
                    None
                };
                if let Some(expr) = compile_expression(expr, unit_encoding, frame_base)? {
                    if expr.is_simple() {
                        if let Some(expr) = expr.build() {
                            write::AttributeValue::Exprloc(expr)
                        } else {
                            continue;
                        }
                    } else {
                        // Conversion to loclist is required.
                        if let Some(scope_ranges) = scope_ranges {
                            let exprs =
                                expr.build_with_locals(scope_ranges, addr_tr, frame_info, endian);
                            if exprs.is_empty() {
                                continue;
                            }
                            let found_single_expr = {
                                // Micro-optimization all expressions alike, use one exprloc.
                                let mut found_expr: Option<write::Expression> = None;
                                for (_, _, expr) in &exprs {
                                    if let Some(ref prev_expr) = found_expr {
                                        if expr.0.eq(&prev_expr.0) {
                                            continue; // the same expression
                                        }
                                        found_expr = None;
                                        break;
                                    }
                                    found_expr = Some(expr.clone())
                                }
                                found_expr
                            };
                            if found_single_expr.is_some() {
                                write::AttributeValue::Exprloc(found_single_expr.unwrap())
                            } else if is_exprloc_to_loclist_allowed(attr.name()) {
                                // Converting exprloc to loclist.
                                let mut locs = Vec::new();
                                for (begin, length, data) in exprs {
                                    if length == 0 {
                                        // Ignore empty range
                                        continue;
                                    }
                                    locs.push(write::Location::StartLength {
                                        begin,
                                        length,
                                        data,
                                    });
                                }
                                let list_id = out_unit.locations.add(write::LocationList(locs));
                                write::AttributeValue::LocationListRef(list_id)
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                } else {
                    // FIXME _expr contains invalid expression
                    continue; // ignore attribute
                }
            }
            AttributeValue::Encoding(e) => write::AttributeValue::Encoding(e),
            AttributeValue::DecimalSign(e) => write::AttributeValue::DecimalSign(e),
            AttributeValue::Endianity(e) => write::AttributeValue::Endianity(e),
            AttributeValue::Accessibility(e) => write::AttributeValue::Accessibility(e),
            AttributeValue::Visibility(e) => write::AttributeValue::Visibility(e),
            AttributeValue::Virtuality(e) => write::AttributeValue::Virtuality(e),
            AttributeValue::Language(e) => write::AttributeValue::Language(e),
            AttributeValue::AddressClass(e) => write::AttributeValue::AddressClass(e),
            AttributeValue::IdentifierCase(e) => write::AttributeValue::IdentifierCase(e),
            AttributeValue::CallingConvention(e) => write::AttributeValue::CallingConvention(e),
            AttributeValue::Inline(e) => write::AttributeValue::Inline(e),
            AttributeValue::Ordering(e) => write::AttributeValue::Ordering(e),
            AttributeValue::UnitRef(ref offset) => {
                if let Some(unit_id) = die_ref_map.get(offset) {
                    write::AttributeValue::ThisUnitEntryRef(*unit_id)
                } else {
                    pending_die_refs.push((current_scope_id, attr.name(), *offset));
                    continue;
                }
            }
            // AttributeValue::DebugInfoRef(_) => {
            //     continue;
            // }
            _ => panic!(), //write::AttributeValue::StringRef(out_strings.add("_")),
        };
        let current_scope = out_unit.get_mut(current_scope_id);
        current_scope.set(attr.name(), attr_value);
    }
    Ok(())
}

pub(crate) fn clone_attr_string<R>(
    attr_value: &AttributeValue<R>,
    form: gimli::DwForm,
    debug_str: &DebugStr<R>,
    out_strings: &mut write::StringTable,
) -> Result<write::LineString, gimli::Error>
where
    R: Reader,
{
    let content = match attr_value {
        AttributeValue::DebugStrRef(str_offset) => {
            debug_str.get_str(*str_offset)?.to_slice()?.to_vec()
        }
        AttributeValue::String(b) => b.to_slice()?.to_vec(),
        _ => panic!("Unexpected attribute value"),
    };
    Ok(match form {
        gimli::DW_FORM_strp => {
            let id = out_strings.add(content);
            write::LineString::StringRef(id)
        }
        gimli::DW_FORM_string => write::LineString::String(content),
        _ => panic!("DW_FORM_line_strp or other not supported"),
    })
}
