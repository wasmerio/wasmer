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
use super::expression::{CompiledExpression, FunctionFrameInfo};
use anyhow::Error;
// TODO: review
use wasmer_runtime_core::types::FuncIndex;
use gimli::write;
use wasmer_runtime_core::{module::ModuleInfo, state::CodeVersion};
// TODO: ValueLabelsRanges

pub(crate) fn add_internal_types(
    comp_unit: &mut write::Unit,
    root_id: write::UnitEntryId,
    out_strings: &mut write::StringTable,
    module_info: &ModuleInfo,
) -> (write::UnitEntryId, write::UnitEntryId) {
    let wp_die_id = comp_unit.add(root_id, gimli::DW_TAG_base_type);
    let wp_die = comp_unit.get_mut(wp_die_id);
    wp_die.set(
        gimli::DW_AT_name,
        write::AttributeValue::StringRef(out_strings.add("WebAssemblyPtr")),
    );
    wp_die.set(gimli::DW_AT_byte_size, write::AttributeValue::Data1(4));
    wp_die.set(
        gimli::DW_AT_encoding,
        write::AttributeValue::Encoding(gimli::DW_ATE_unsigned),
    );

    let memory_byte_die_id = comp_unit.add(root_id, gimli::DW_TAG_base_type);
    let memory_byte_die = comp_unit.get_mut(memory_byte_die_id);
    memory_byte_die.set(
        gimli::DW_AT_name,
        write::AttributeValue::StringRef(out_strings.add("u8")),
    );
    memory_byte_die.set(
        gimli::DW_AT_encoding,
        write::AttributeValue::Encoding(gimli::DW_ATE_unsigned),
    );
    memory_byte_die.set(gimli::DW_AT_byte_size, write::AttributeValue::Data1(1));

    let memory_bytes_die_id = comp_unit.add(root_id, gimli::DW_TAG_pointer_type);
    let memory_bytes_die = comp_unit.get_mut(memory_bytes_die_id);
    memory_bytes_die.set(
        gimli::DW_AT_name,
        write::AttributeValue::StringRef(out_strings.add("u8*")),
    );
    memory_bytes_die.set(
        gimli::DW_AT_type,
        write::AttributeValue::ThisUnitEntryRef(memory_byte_die_id),
    );

    let memory_offset = unimplemented!("TODO");
    let vmctx_die_id = comp_unit.add(root_id, gimli::DW_TAG_structure_type);
    let vmctx_die = comp_unit.get_mut(vmctx_die_id);
    vmctx_die.set(
        gimli::DW_AT_name,
        write::AttributeValue::StringRef(out_strings.add("WasmerVMContext")),
    );
    vmctx_die.set(
        gimli::DW_AT_byte_size,
        write::AttributeValue::Data4(memory_offset as u32 + 8),
    );

    let m_die_id = comp_unit.add(vmctx_die_id, gimli::DW_TAG_member);
    let m_die = comp_unit.get_mut(m_die_id);
    m_die.set(
        gimli::DW_AT_name,
        write::AttributeValue::StringRef(out_strings.add("memory")),
    );
    m_die.set(
        gimli::DW_AT_type,
        write::AttributeValue::ThisUnitEntryRef(memory_bytes_die_id),
    );
    m_die.set(
        gimli::DW_AT_data_member_location,
        write::AttributeValue::Udata(memory_offset as u64),
    );

    let vmctx_ptr_die_id = comp_unit.add(root_id, gimli::DW_TAG_pointer_type);
    let vmctx_ptr_die = comp_unit.get_mut(vmctx_ptr_die_id);
    vmctx_ptr_die.set(
        gimli::DW_AT_name,
        write::AttributeValue::StringRef(out_strings.add("WasmerVMContext*")),
    );
    vmctx_ptr_die.set(
        gimli::DW_AT_type,
        write::AttributeValue::ThisUnitEntryRef(vmctx_die_id),
    );

    (wp_die_id, vmctx_ptr_die_id)
}

pub(crate) fn append_vmctx_info(
    comp_unit: &mut write::Unit,
    parent_id: write::UnitEntryId,
    vmctx_die_id: write::UnitEntryId,
    addr_tr: &AddressTransform,
    frame_info: Option<&FunctionFrameInfo>,
    scope_ranges: &[(u64, u64)],
    out_strings: &mut write::StringTable,
) -> Result<(), Error> {
    let loc = {
        let endian = gimli::RunTimeEndian::Little;

        let expr = CompiledExpression::vmctx();
        let mut locs = Vec::new();
        for (begin, length, data) in
            expr.build_with_locals(scope_ranges, addr_tr, frame_info, endian)
        {
            locs.push(write::Location::StartLength {
                begin,
                length,
                data,
            });
        }
        let list_id = comp_unit.locations.add(write::LocationList(locs));
        write::AttributeValue::LocationListRef(list_id)
    };

    let var_die_id = comp_unit.add(parent_id, gimli::DW_TAG_variable);
    let var_die = comp_unit.get_mut(var_die_id);
    var_die.set(
        gimli::DW_AT_name,
        write::AttributeValue::StringRef(out_strings.add("__vmctx")),
    );
    var_die.set(
        gimli::DW_AT_type,
        write::AttributeValue::ThisUnitEntryRef(vmctx_die_id),
    );
    var_die.set(gimli::DW_AT_location, loc);

    Ok(())
}

pub(crate) fn get_function_frame_info<'a, 'b, 'c>(
    module_info: &'b ModuleInfo,
    func_index: FuncIndex,
    value_ranges: &'c ValueLabelsRanges,
) -> Option<FunctionFrameInfo<'a>>
where
    'b: 'a,
    'c: 'a,
{
    if let Some(value_ranges) = value_ranges.get(func_index) {
        let frame_info = FunctionFrameInfo {
            value_ranges,
            memory_offset: module_info.memory_offset,
            stack_slots: &module_info.stack_slots[func_index],
        };
        Some(frame_info)
    } else {
        None
    }
}
