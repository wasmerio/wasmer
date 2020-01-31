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
use super::{DebugInputContext, Reader};
use anyhow::Error;
use cranelift_entity::EntityRef;
use wasmer_runtime_core::types::FuncIndex
use gimli::{write, AttributeValue, DebuggingInformationEntry, RangeListsOffset};

pub(crate) enum RangeInfoBuilder {
    Undefined,
    Position(u64),
    Ranges(Vec<(u64, u64)>),
    Function(FuncIndex),
}

impl RangeInfoBuilder {
    pub(crate) fn from<R>(
        entry: &DebuggingInformationEntry<R>,
        context: &DebugInputContext<R>,
        unit_encoding: gimli::Encoding,
        cu_low_pc: u64,
    ) -> Result<Self, Error>
    where
        R: Reader,
    {
        if let Some(AttributeValue::RangeListsRef(r)) = entry.attr_value(gimli::DW_AT_ranges)? {
            return RangeInfoBuilder::from_ranges_ref(r, context, unit_encoding, cu_low_pc);
        };

        let low_pc =
            if let Some(AttributeValue::Addr(addr)) = entry.attr_value(gimli::DW_AT_low_pc)? {
                addr
            } else {
                return Ok(RangeInfoBuilder::Undefined);
            };

        Ok(
            if let Some(AttributeValue::Udata(u)) = entry.attr_value(gimli::DW_AT_high_pc)? {
                RangeInfoBuilder::Ranges(vec![(low_pc, low_pc + u)])
            } else {
                RangeInfoBuilder::Position(low_pc)
            },
        )
    }

    pub(crate) fn from_ranges_ref<R>(
        ranges: RangeListsOffset,
        context: &DebugInputContext<R>,
        unit_encoding: gimli::Encoding,
        cu_low_pc: u64,
    ) -> Result<Self, Error>
    where
        R: Reader,
    {
        let mut ranges = context.rnglists.ranges(
            ranges,
            unit_encoding,
            cu_low_pc,
            &context.debug_addr,
            context.debug_addr_base,
        )?;
        let mut result = Vec::new();
        while let Some(range) = ranges.next()? {
            if range.begin >= range.end {
                // ignore empty ranges
            }
            result.push((range.begin, range.end));
        }

        Ok(if result.len() > 0 {
            RangeInfoBuilder::Ranges(result)
        } else {
            RangeInfoBuilder::Undefined
        })
    }

    pub(crate) fn from_subprogram_die<R>(
        entry: &DebuggingInformationEntry<R>,
        context: &DebugInputContext<R>,
        unit_encoding: gimli::Encoding,
        addr_tr: &AddressTransform,
        cu_low_pc: u64,
    ) -> Result<Self, Error>
    where
        R: Reader,
    {
        let addr =
            if let Some(AttributeValue::Addr(addr)) = entry.attr_value(gimli::DW_AT_low_pc)? {
                addr
            } else if let Some(AttributeValue::RangeListsRef(r)) =
                entry.attr_value(gimli::DW_AT_ranges)?
            {
                let mut ranges = context.rnglists.ranges(
                    r,
                    unit_encoding,
                    cu_low_pc,
                    &context.debug_addr,
                    context.debug_addr_base,
                )?;
                if let Some(range) = ranges.next()? {
                    range.begin
                } else {
                    return Ok(RangeInfoBuilder::Undefined);
                }
            } else {
                return Ok(RangeInfoBuilder::Undefined);
            };

        let index = addr_tr.find_func_index(addr);
        if index.is_none() {
            return Ok(RangeInfoBuilder::Undefined);
        }
        Ok(RangeInfoBuilder::Function(index.unwrap()))
    }

    pub(crate) fn build(
        &self,
        addr_tr: &AddressTransform,
        out_unit: &mut write::Unit,
        current_scope_id: write::UnitEntryId,
    ) {
        match self {
            RangeInfoBuilder::Undefined => (),
            RangeInfoBuilder::Position(pc) => {
                let addr = addr_tr
                    .translate(*pc)
                    .unwrap_or(write::Address::Constant(0));
                let current_scope = out_unit.get_mut(current_scope_id);
                current_scope.set(gimli::DW_AT_low_pc, write::AttributeValue::Address(addr));
            }
            RangeInfoBuilder::Ranges(ranges) => {
                let mut result = Vec::new();
                for (begin, end) in ranges {
                    for tr in addr_tr.translate_ranges(*begin, *end) {
                        if tr.1 == 0 {
                            // Ignore empty range
                            continue;
                        }
                        result.push(tr);
                    }
                }
                if result.len() != 1 {
                    let range_list = result
                        .iter()
                        .map(|tr| write::Range::StartLength {
                            begin: tr.0,
                            length: tr.1,
                        })
                        .collect::<Vec<_>>();
                    let range_list_id = out_unit.ranges.add(write::RangeList(range_list));
                    let current_scope = out_unit.get_mut(current_scope_id);
                    current_scope.set(
                        gimli::DW_AT_ranges,
                        write::AttributeValue::RangeListRef(range_list_id),
                    );
                } else {
                    let current_scope = out_unit.get_mut(current_scope_id);
                    current_scope.set(
                        gimli::DW_AT_low_pc,
                        write::AttributeValue::Address(result[0].0),
                    );
                    current_scope.set(
                        gimli::DW_AT_high_pc,
                        write::AttributeValue::Udata(result[0].1),
                    );
                }
            }
            RangeInfoBuilder::Function(index) => {
                let range = addr_tr.func_range(*index);
                let symbol = index.index();
                let addr = write::Address::Symbol {
                    symbol,
                    addend: range.0 as i64,
                };
                let len = (range.1 - range.0) as u64;
                let current_scope = out_unit.get_mut(current_scope_id);
                current_scope.set(gimli::DW_AT_low_pc, write::AttributeValue::Address(addr));
                current_scope.set(gimli::DW_AT_high_pc, write::AttributeValue::Udata(len));
            }
        }
    }

    pub(crate) fn get_ranges(&self, addr_tr: &AddressTransform) -> Vec<(u64, u64)> {
        match self {
            RangeInfoBuilder::Undefined | RangeInfoBuilder::Position(_) => vec![],
            RangeInfoBuilder::Ranges(ranges) => ranges.clone(),
            RangeInfoBuilder::Function(index) => {
                let range = addr_tr.func_source_range(*index);
                vec![(range.0, range.1)]
            }
        }
    }

    pub(crate) fn build_ranges(
        &self,
        addr_tr: &AddressTransform,
        out_range_lists: &mut write::RangeListTable,
    ) -> write::RangeListId {
        if let RangeInfoBuilder::Ranges(ranges) = self {
            let mut range_list = Vec::new();
            for (begin, end) in ranges {
                assert!(begin < end);
                for tr in addr_tr.translate_ranges(*begin, *end) {
                    if tr.1 == 0 {
                        // Ignore empty range
                        continue;
                    }
                    range_list.push(write::Range::StartLength {
                        begin: tr.0,
                        length: tr.1,
                    });
                }
            }
            out_range_lists.add(write::RangeList(range_list))
        } else {
            unreachable!();
        }
    }
}
