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
// It was copied at revision `39e57e3e9ac9c15bef45eb77a2544a7c0b76501a`.
//
// Changes to this file are copyright of Wasmer inc. unless otherwise indicated
// and are licensed under the Wasmer project's license.
use crate::transform::AddressTransform;
use gimli::constants;
use gimli::read;
use gimli::{Reader, UnitSectionOffset};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct Dependencies {
    edges: HashMap<UnitSectionOffset, HashSet<UnitSectionOffset>>,
    roots: HashSet<UnitSectionOffset>,
}

impl Dependencies {
    fn new() -> Dependencies {
        Dependencies {
            edges: HashMap::new(),
            roots: HashSet::new(),
        }
    }

    fn add_edge(&mut self, a: UnitSectionOffset, b: UnitSectionOffset) {
        use std::collections::hash_map::Entry;
        match self.edges.entry(a) {
            Entry::Occupied(mut o) => {
                o.get_mut().insert(b);
            }
            Entry::Vacant(v) => {
                let mut set = HashSet::new();
                set.insert(b);
                v.insert(set);
            }
        }
    }

    fn add_root(&mut self, root: UnitSectionOffset) {
        self.roots.insert(root);
    }

    pub fn get_reachable(&self) -> HashSet<UnitSectionOffset> {
        let mut reachable = self.roots.clone();
        let mut queue = Vec::new();
        for i in self.roots.iter() {
            if let Some(deps) = self.edges.get(i) {
                for j in deps {
                    if reachable.contains(j) {
                        continue;
                    }
                    reachable.insert(*j);
                    queue.push(*j);
                }
            }
        }
        while let Some(i) = queue.pop() {
            if let Some(deps) = self.edges.get(&i) {
                for j in deps {
                    if reachable.contains(j) {
                        continue;
                    }
                    reachable.insert(*j);
                    queue.push(*j);
                }
            }
        }
        reachable
    }
}

pub fn build_dependencies<R: Reader<Offset = usize>>(
    dwarf: &read::Dwarf<R>,
    at: &AddressTransform,
) -> read::Result<Dependencies> {
    let mut deps = Dependencies::new();
    let mut units = dwarf.units();
    while let Some(unit) = units.next()? {
        build_unit_dependencies(unit, dwarf, at, &mut deps)?;
    }
    Ok(deps)
}

fn build_unit_dependencies<R: Reader<Offset = usize>>(
    header: read::CompilationUnitHeader<R>,
    dwarf: &read::Dwarf<R>,
    at: &AddressTransform,
    deps: &mut Dependencies,
) -> read::Result<()> {
    let unit = dwarf.unit(header)?;
    let mut tree = unit.entries_tree(None)?;
    let root = tree.root()?;
    build_die_dependencies(root, dwarf, &unit, at, deps)?;
    Ok(())
}

fn has_die_back_edge<R: Reader<Offset = usize>>(die: &read::DebuggingInformationEntry<R>) -> bool {
    match die.tag() {
        constants::DW_TAG_variable
        | constants::DW_TAG_constant
        | constants::DW_TAG_inlined_subroutine
        | constants::DW_TAG_lexical_block
        | constants::DW_TAG_label
        | constants::DW_TAG_with_stmt
        | constants::DW_TAG_try_block
        | constants::DW_TAG_catch_block
        | constants::DW_TAG_template_type_parameter
        | constants::DW_TAG_member
        | constants::DW_TAG_formal_parameter => true,
        _ => false,
    }
}

fn has_valid_code_range<R: Reader<Offset = usize>>(
    die: &read::DebuggingInformationEntry<R>,
    dwarf: &read::Dwarf<R>,
    unit: &read::Unit<R>,
    at: &AddressTransform,
) -> read::Result<bool> {
    match die.tag() {
        constants::DW_TAG_subprogram => {
            if let Some(ranges_attr) = die.attr_value(constants::DW_AT_ranges)? {
                let offset = match ranges_attr {
                    read::AttributeValue::RangeListsRef(val) => val,
                    read::AttributeValue::DebugRngListsIndex(index) => {
                        dwarf.ranges_offset(unit, index)?
                    }
                    _ => return Ok(false),
                };
                let mut has_valid_base = if let Some(read::AttributeValue::Addr(low_pc)) =
                    die.attr_value(constants::DW_AT_low_pc)?
                {
                    Some(at.can_translate_address(low_pc))
                } else {
                    None
                };
                let mut it = dwarf.ranges.raw_ranges(offset, unit.encoding())?;
                while let Some(range) = it.next()? {
                    // If at least one of the range addresses can be converted,
                    // declaring code range as valid.
                    match range {
                        read::RawRngListEntry::AddressOrOffsetPair { .. }
                            if has_valid_base.is_some() =>
                        {
                            if has_valid_base.unwrap() {
                                return Ok(true);
                            }
                        }
                        read::RawRngListEntry::StartEnd { begin, .. }
                        | read::RawRngListEntry::StartLength { begin, .. }
                        | read::RawRngListEntry::AddressOrOffsetPair { begin, .. } => {
                            if at.can_translate_address(begin) {
                                return Ok(true);
                            }
                        }
                        read::RawRngListEntry::StartxEndx { begin, .. }
                        | read::RawRngListEntry::StartxLength { begin, .. } => {
                            let addr = dwarf.address(unit, begin)?;
                            if at.can_translate_address(addr) {
                                return Ok(true);
                            }
                        }
                        read::RawRngListEntry::BaseAddress { addr } => {
                            has_valid_base = Some(at.can_translate_address(addr));
                        }
                        read::RawRngListEntry::BaseAddressx { addr } => {
                            let addr = dwarf.address(unit, addr)?;
                            has_valid_base = Some(at.can_translate_address(addr));
                        }
                        read::RawRngListEntry::OffsetPair { .. } => (),
                    }
                }
                return Ok(false);
            } else if let Some(low_pc) = die.attr_value(constants::DW_AT_low_pc)? {
                if let read::AttributeValue::Addr(a) = low_pc {
                    return Ok(at.can_translate_address(a));
                }
            }
        }
        _ => (),
    }
    Ok(false)
}

fn build_die_dependencies<R: Reader<Offset = usize>>(
    die: read::EntriesTreeNode<R>,
    dwarf: &read::Dwarf<R>,
    unit: &read::Unit<R>,
    at: &AddressTransform,
    deps: &mut Dependencies,
) -> read::Result<()> {
    let entry = die.entry();
    let offset = entry.offset().to_unit_section_offset(unit);
    let mut attrs = entry.attrs();
    while let Some(attr) = attrs.next()? {
        build_attr_dependencies(&attr, offset, dwarf, unit, at, deps)?;
    }

    let mut children = die.children();
    while let Some(child) = children.next()? {
        let child_entry = child.entry();
        let child_offset = child_entry.offset().to_unit_section_offset(unit);
        deps.add_edge(child_offset, offset);
        if has_die_back_edge(child_entry) {
            deps.add_edge(offset, child_offset);
        }
        if has_valid_code_range(child_entry, dwarf, unit, at)? {
            deps.add_root(child_offset);
        }
        build_die_dependencies(child, dwarf, unit, at, deps)?;
    }
    Ok(())
}

fn build_attr_dependencies<R: Reader<Offset = usize>>(
    attr: &read::Attribute<R>,
    offset: UnitSectionOffset,
    _dwarf: &read::Dwarf<R>,
    unit: &read::Unit<R>,
    _at: &AddressTransform,
    deps: &mut Dependencies,
) -> read::Result<()> {
    match attr.value() {
        read::AttributeValue::UnitRef(val) => {
            let ref_offset = val.to_unit_section_offset(unit);
            deps.add_edge(offset, ref_offset);
        }
        read::AttributeValue::DebugInfoRef(val) => {
            let ref_offset = UnitSectionOffset::DebugInfoOffset(val);
            deps.add_edge(offset, ref_offset);
        }
        _ => (),
    }
    Ok(())
}
