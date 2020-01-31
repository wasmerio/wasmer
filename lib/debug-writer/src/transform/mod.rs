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
use crate::gc::build_dependencies;
use crate::DebugInfoData;
use anyhow::Error;
use cranelift_codegen::isa::TargetFrontendConfig;
use gimli::{
    write, DebugAddr, DebugAddrBase, DebugLine, DebugStr, LocationLists, RangeLists,
    UnitSectionOffset,
};
use simulate::generate_simulated_dwarf;
use std::collections::HashSet;
use thiserror::Error;
use unit::clone_unit;
use wasmtime_environ::{ModuleAddressMap, ModuleVmctxInfo, ValueLabelsRanges};

pub use address_transform::AddressTransform;

mod address_transform;
mod attr;
mod expression;
mod line_program;
mod range_info_builder;
mod simulate;
mod unit;
mod utils;

pub(crate) trait Reader: gimli::Reader<Offset = usize> {}

impl<'input, Endian> Reader for gimli::EndianSlice<'input, Endian> where Endian: gimli::Endianity {}

#[derive(Error, Debug)]
#[error("Debug info transform error: {0}")]
pub struct TransformError(&'static str);

pub(crate) struct DebugInputContext<'a, R>
where
    R: Reader,
{
    debug_str: &'a DebugStr<R>,
    debug_line: &'a DebugLine<R>,
    debug_addr: &'a DebugAddr<R>,
    debug_addr_base: DebugAddrBase<R::Offset>,
    rnglists: &'a RangeLists<R>,
    loclists: &'a LocationLists<R>,
    reachable: &'a HashSet<UnitSectionOffset>,
}

pub fn transform_dwarf(
    target_config: &TargetFrontendConfig,
    di: &DebugInfoData,
    at: &ModuleAddressMap,
    vmctx_info: &ModuleVmctxInfo,
    ranges: &ValueLabelsRanges,
) -> Result<write::Dwarf, Error> {
    let addr_tr = AddressTransform::new(at, &di.wasm_file);
    let reachable = build_dependencies(&di.dwarf, &addr_tr)?.get_reachable();

    let context = DebugInputContext {
        debug_str: &di.dwarf.debug_str,
        debug_line: &di.dwarf.debug_line,
        debug_addr: &di.dwarf.debug_addr,
        debug_addr_base: DebugAddrBase(0),
        rnglists: &di.dwarf.ranges,
        loclists: &di.dwarf.locations,
        reachable: &reachable,
    };

    let out_encoding = gimli::Encoding {
        format: gimli::Format::Dwarf32,
        // TODO: this should be configurable
        // macOS doesn't seem to support DWARF > 3
        version: 3,
        address_size: target_config.pointer_bytes(),
    };

    let mut out_strings = write::StringTable::default();
    let mut out_units = write::UnitTable::default();

    let out_line_strings = write::LineStringTable::default();

    let mut translated = HashSet::new();
    let mut iter = di.dwarf.debug_info.units();
    while let Some(unit) = iter.next().unwrap_or(None) {
        let unit = di.dwarf.unit(unit)?;
        clone_unit(
            unit,
            &context,
            &addr_tr,
            &ranges,
            out_encoding,
            &vmctx_info,
            &mut out_units,
            &mut out_strings,
            &mut translated,
        )?;
    }

    generate_simulated_dwarf(
        &addr_tr,
        di,
        &vmctx_info,
        &ranges,
        &translated,
        out_encoding,
        &mut out_units,
        &mut out_strings,
    )?;

    Ok(write::Dwarf {
        units: out_units,
        line_programs: vec![],
        line_strings: out_line_strings,
        strings: out_strings,
    })
}
