//   Copyright 2029 WasmTime Project Developers
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
// It was copied at revision `3992b8669f9b9e185abe81e9998ce2ff4d40ff68`.
//
// Changes to this file are copyright of Wasmer inc. unless otherwise indicated
// and are licensed under the Wasmer project's license.
use super::address_transform::AddressTransform;
use anyhow::Error;
use cranelift_codegen::ir::{StackSlots, ValueLabel, ValueLoc};
use cranelift_codegen::isa::RegUnit;
use cranelift_codegen::ValueLabelsRanges;
use cranelift_entity::EntityRef;
use cranelift_wasm::{get_vmctx_value_label, DefinedFuncIndex};
use gimli::{self, write, Expression, Operation, Reader, ReaderOffset, Register, X86_64};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct FunctionFrameInfo<'a> {
    pub value_ranges: &'a ValueLabelsRanges,
    pub memory_offset: i64,
    pub stack_slots: &'a StackSlots,
}

#[derive(Debug)]
enum CompiledExpressionPart {
    Code(Vec<u8>),
    Local(ValueLabel),
    Deref,
}

#[derive(Debug)]
pub struct CompiledExpression {
    parts: Vec<CompiledExpressionPart>,
    need_deref: bool,
}

impl Clone for CompiledExpressionPart {
    fn clone(&self) -> Self {
        match self {
            CompiledExpressionPart::Code(c) => CompiledExpressionPart::Code(c.clone()),
            CompiledExpressionPart::Local(i) => CompiledExpressionPart::Local(*i),
            CompiledExpressionPart::Deref => CompiledExpressionPart::Deref,
        }
    }
}

impl CompiledExpression {
    pub fn vmctx() -> CompiledExpression {
        CompiledExpression::from_label(get_vmctx_value_label())
    }

    pub fn from_label(label: ValueLabel) -> CompiledExpression {
        CompiledExpression {
            parts: vec![
                CompiledExpressionPart::Local(label),
                CompiledExpressionPart::Code(vec![gimli::constants::DW_OP_stack_value.0 as u8]),
            ],
            need_deref: false,
        }
    }
}

fn map_reg(reg: RegUnit) -> Register {
    static mut REG_X86_MAP: Option<HashMap<RegUnit, Register>> = None;
    // FIXME lazy initialization?
    unsafe {
        if REG_X86_MAP.is_none() {
            REG_X86_MAP = Some(HashMap::new());
        }
        if let Some(val) = REG_X86_MAP.as_mut().unwrap().get(&reg) {
            return *val;
        }
        let result = match reg {
            0 => X86_64::RAX,
            1 => X86_64::RCX,
            2 => X86_64::RDX,
            3 => X86_64::RBX,
            4 => X86_64::RSP,
            5 => X86_64::RBP,
            6 => X86_64::RSI,
            7 => X86_64::RDI,
            8 => X86_64::R8,
            9 => X86_64::R9,
            10 => X86_64::R10,
            11 => X86_64::R11,
            12 => X86_64::R12,
            13 => X86_64::R13,
            14 => X86_64::R14,
            15 => X86_64::R15,
            16 => X86_64::XMM0,
            17 => X86_64::XMM1,
            18 => X86_64::XMM2,
            19 => X86_64::XMM3,
            20 => X86_64::XMM4,
            21 => X86_64::XMM5,
            22 => X86_64::XMM6,
            23 => X86_64::XMM7,
            24 => X86_64::XMM8,
            25 => X86_64::XMM9,
            26 => X86_64::XMM10,
            27 => X86_64::XMM11,
            28 => X86_64::XMM12,
            29 => X86_64::XMM13,
            30 => X86_64::XMM14,
            31 => X86_64::XMM15,
            _ => panic!("unknown x86_64 register {}", reg),
        };
        REG_X86_MAP.as_mut().unwrap().insert(reg, result);
        result
    }
}

fn translate_loc(loc: ValueLoc, frame_info: Option<&FunctionFrameInfo>) -> Option<Vec<u8>> {
    match loc {
        ValueLoc::Reg(reg) => {
            let machine_reg = map_reg(reg).0 as u8;
            assert!(machine_reg < 32); // FIXME
            Some(vec![gimli::constants::DW_OP_reg0.0 + machine_reg])
        }
        ValueLoc::Stack(ss) => {
            if let Some(frame_info) = frame_info {
                if let Some(ss_offset) = frame_info.stack_slots[ss].offset {
                    use gimli::write::Writer;
                    let endian = gimli::RunTimeEndian::Little;
                    let mut writer = write::EndianVec::new(endian);
                    writer
                        .write_u8(gimli::constants::DW_OP_breg0.0 + X86_64::RBP.0 as u8)
                        .expect("bp wr");
                    writer.write_sleb128(ss_offset as i64 + 16).expect("ss wr");
                    writer
                        .write_u8(gimli::constants::DW_OP_deref.0 as u8)
                        .expect("bp wr");
                    let buf = writer.into_vec();
                    return Some(buf);
                }
            }
            None
        }
        _ => None,
    }
}

fn append_memory_deref(
    buf: &mut Vec<u8>,
    frame_info: &FunctionFrameInfo,
    vmctx_loc: ValueLoc,
    endian: gimli::RunTimeEndian,
) -> write::Result<bool> {
    use gimli::write::Writer;
    let mut writer = write::EndianVec::new(endian);
    match vmctx_loc {
        ValueLoc::Reg(vmctx_reg) => {
            let reg = map_reg(vmctx_reg);
            writer.write_u8(gimli::constants::DW_OP_breg0.0 + reg.0 as u8)?;
            writer.write_sleb128(frame_info.memory_offset)?;
        }
        ValueLoc::Stack(ss) => {
            if let Some(ss_offset) = frame_info.stack_slots[ss].offset {
                writer.write_u8(gimli::constants::DW_OP_breg0.0 + X86_64::RBP.0 as u8)?;
                writer.write_sleb128(ss_offset as i64 + 16)?;
                writer.write_u8(gimli::constants::DW_OP_deref.0 as u8)?;

                writer.write_u8(gimli::constants::DW_OP_consts.0 as u8)?;
                writer.write_sleb128(frame_info.memory_offset)?;
                writer.write_u8(gimli::constants::DW_OP_plus.0 as u8)?;
            } else {
                return Ok(false);
            }
        }
        _ => {
            return Ok(false);
        }
    }
    writer.write_u8(gimli::constants::DW_OP_deref.0 as u8)?;
    writer.write_u8(gimli::constants::DW_OP_swap.0 as u8)?;
    writer.write_u8(gimli::constants::DW_OP_stack_value.0 as u8)?;
    writer.write_u8(gimli::constants::DW_OP_constu.0 as u8)?;
    writer.write_uleb128(0xffff_ffff)?;
    writer.write_u8(gimli::constants::DW_OP_and.0 as u8)?;
    writer.write_u8(gimli::constants::DW_OP_plus.0 as u8)?;
    buf.extend_from_slice(writer.slice());
    Ok(true)
}

impl CompiledExpression {
    pub fn is_simple(&self) -> bool {
        if let [CompiledExpressionPart::Code(_)] = self.parts.as_slice() {
            true
        } else {
            self.parts.is_empty()
        }
    }

    pub fn build(&self) -> Option<write::Expression> {
        if let [CompiledExpressionPart::Code(code)] = self.parts.as_slice() {
            return Some(write::Expression(code.to_vec()));
        }
        // locals found, not supported
        None
    }

    pub fn build_with_locals(
        &self,
        scope: &[(u64, u64)], // wasm ranges
        addr_tr: &AddressTransform,
        frame_info: Option<&FunctionFrameInfo>,
        endian: gimli::RunTimeEndian,
    ) -> Vec<(write::Address, u64, write::Expression)> {
        if scope.is_empty() {
            return vec![];
        }

        if let [CompiledExpressionPart::Code(code)] = self.parts.as_slice() {
            let mut result_scope = Vec::new();
            for s in scope {
                for (addr, len) in addr_tr.translate_ranges(s.0, s.1) {
                    result_scope.push((addr, len, write::Expression(code.to_vec())));
                }
            }
            return result_scope;
        }

        let vmctx_label = get_vmctx_value_label();

        // Some locals are present, preparing and divided ranges based on the scope
        // and frame_info data.
        let mut ranges_builder = ValueLabelRangesBuilder::new(scope, addr_tr, frame_info);
        for p in &self.parts {
            match p {
                CompiledExpressionPart::Code(_) => (),
                CompiledExpressionPart::Local(label) => ranges_builder.process_label(*label),
                CompiledExpressionPart::Deref => ranges_builder.process_label(vmctx_label),
            }
        }
        if self.need_deref {
            ranges_builder.process_label(vmctx_label);
        }
        ranges_builder.remove_incomplete_ranges();
        let ranges = ranges_builder.ranges;

        let mut result = Vec::new();
        'range: for CachedValueLabelRange {
            func_index,
            start,
            end,
            label_location,
        } in ranges
        {
            // build expression
            let mut code_buf = Vec::new();
            for part in &self.parts {
                match part {
                    CompiledExpressionPart::Code(c) => code_buf.extend_from_slice(c.as_slice()),
                    CompiledExpressionPart::Local(label) => {
                        let loc = *label_location.get(&label).expect("loc");
                        if let Some(expr) = translate_loc(loc, frame_info) {
                            code_buf.extend_from_slice(&expr)
                        } else {
                            continue 'range;
                        }
                    }
                    CompiledExpressionPart::Deref => {
                        if let (Some(vmctx_loc), Some(frame_info)) =
                            (label_location.get(&vmctx_label), frame_info)
                        {
                            if !append_memory_deref(&mut code_buf, frame_info, *vmctx_loc, endian)
                                .expect("append_memory_deref")
                            {
                                continue 'range;
                            }
                        } else {
                            continue 'range;
                        };
                    }
                }
            }
            if self.need_deref {
                if let (Some(vmctx_loc), Some(frame_info)) =
                    (label_location.get(&vmctx_label), frame_info)
                {
                    if !append_memory_deref(&mut code_buf, frame_info, *vmctx_loc, endian)
                        .expect("append_memory_deref")
                    {
                        continue 'range;
                    }
                } else {
                    continue 'range;
                };
            }
            result.push((
                write::Address::Symbol {
                    symbol: func_index.index(),
                    addend: start as i64,
                },
                (end - start) as u64,
                write::Expression(code_buf),
            ));
        }

        result
    }
}

pub fn compile_expression<R>(
    expr: &Expression<R>,
    encoding: gimli::Encoding,
    frame_base: Option<&CompiledExpression>,
) -> Result<Option<CompiledExpression>, Error>
where
    R: Reader,
{
    let mut parts = Vec::new();
    let mut need_deref = false;
    if let Some(frame_base) = frame_base {
        parts.extend_from_slice(&frame_base.parts);
        need_deref = frame_base.need_deref;
    }
    let base_len = parts.len();
    let mut pc = expr.0.clone();
    let mut code_chunk = Vec::new();
    let buf = expr.0.to_slice()?;
    while !pc.is_empty() {
        let next = buf[pc.offset_from(&expr.0).into_u64() as usize];
        need_deref = true;
        if next == 0xED {
            // WebAssembly DWARF extension
            pc.read_u8()?;
            let ty = pc.read_uleb128()?;
            assert_eq!(ty, 0);
            let index = pc.read_sleb128()?;
            pc.read_u8()?; // consume 159
            if code_chunk.len() > 0 {
                parts.push(CompiledExpressionPart::Code(code_chunk));
                code_chunk = Vec::new();
            }
            let label = ValueLabel::from_u32(index as u32);
            parts.push(CompiledExpressionPart::Local(label));
        } else {
            let pos = pc.offset_from(&expr.0).into_u64() as usize;
            let op = Operation::parse(&mut pc, &expr.0, encoding)?;
            match op {
                Operation::Literal { .. } | Operation::PlusConstant { .. } => (),
                Operation::StackValue => {
                    need_deref = false;
                }
                Operation::Deref { .. } => {
                    if code_chunk.len() > 0 {
                        parts.push(CompiledExpressionPart::Code(code_chunk));
                        code_chunk = Vec::new();
                    }
                    parts.push(CompiledExpressionPart::Deref);
                }
                _ => {
                    return Ok(None);
                }
            }
            let chunk = &buf[pos..pc.offset_from(&expr.0).into_u64() as usize];
            code_chunk.extend_from_slice(chunk);
        }
    }

    if code_chunk.len() > 0 {
        parts.push(CompiledExpressionPart::Code(code_chunk));
    }

    if base_len > 0 && base_len + 1 < parts.len() {
        // see if we can glue two code chunks
        if let [CompiledExpressionPart::Code(cc1), CompiledExpressionPart::Code(cc2)] =
            &parts[base_len..base_len + 1]
        {
            let mut combined = cc1.clone();
            combined.extend_from_slice(cc2);
            parts[base_len] = CompiledExpressionPart::Code(combined);
            parts.remove(base_len + 1);
        }
    }

    Ok(Some(CompiledExpression { parts, need_deref }))
}

#[derive(Debug, Clone)]
struct CachedValueLabelRange {
    func_index: DefinedFuncIndex,
    start: usize,
    end: usize,
    label_location: HashMap<ValueLabel, ValueLoc>,
}

struct ValueLabelRangesBuilder<'a, 'b> {
    ranges: Vec<CachedValueLabelRange>,
    addr_tr: &'a AddressTransform,
    frame_info: Option<&'a FunctionFrameInfo<'b>>,
    processed_labels: HashSet<ValueLabel>,
}

impl<'a, 'b> ValueLabelRangesBuilder<'a, 'b> {
    fn new(
        scope: &[(u64, u64)], // wasm ranges
        addr_tr: &'a AddressTransform,
        frame_info: Option<&'a FunctionFrameInfo<'b>>,
    ) -> Self {
        let mut ranges = Vec::new();
        for s in scope {
            if let Some((func_index, tr)) = addr_tr.translate_ranges_raw(s.0, s.1) {
                for (start, end) in tr {
                    ranges.push(CachedValueLabelRange {
                        func_index,
                        start,
                        end,
                        label_location: HashMap::new(),
                    })
                }
            }
        }
        ranges.sort_unstable_by(|a, b| a.start.cmp(&b.start));
        ValueLabelRangesBuilder {
            ranges,
            addr_tr,
            frame_info,
            processed_labels: HashSet::new(),
        }
    }

    fn process_label(&mut self, label: ValueLabel) {
        if self.processed_labels.contains(&label) {
            return;
        }
        self.processed_labels.insert(label);

        let value_ranges = if let Some(frame_info) = self.frame_info {
            &frame_info.value_ranges
        } else {
            return;
        };

        let ranges = &mut self.ranges;
        if let Some(local_ranges) = value_ranges.get(&label) {
            for local_range in local_ranges {
                let wasm_start = local_range.start;
                let wasm_end = local_range.end;
                let loc = local_range.loc;
                // Find all native ranges for the value label ranges.
                for (addr, len) in self
                    .addr_tr
                    .translate_ranges(wasm_start as u64, wasm_end as u64)
                {
                    let (range_start, range_end) = self.addr_tr.convert_to_code_range(addr, len);
                    if range_start == range_end {
                        continue;
                    }
                    assert!(range_start < range_end);
                    // Find acceptable scope of ranges to intersect with.
                    let i = match ranges.binary_search_by(|s| s.start.cmp(&range_start)) {
                        Ok(i) => i,
                        Err(i) => {
                            if i > 0 && range_start < ranges[i - 1].end {
                                i - 1
                            } else {
                                i
                            }
                        }
                    };
                    let j = match ranges.binary_search_by(|s| s.start.cmp(&range_end)) {
                        Ok(i) | Err(i) => i,
                    };
                    // Starting for the end, intersect (range_start..range_end) with
                    // self.ranges array.
                    for i in (i..j).rev() {
                        if range_end <= ranges[i].start || ranges[i].end <= range_start {
                            continue;
                        }
                        if range_end < ranges[i].end {
                            // Cutting some of the range from the end.
                            let mut tail = ranges[i].clone();
                            ranges[i].end = range_end;
                            tail.start = range_end;
                            ranges.insert(i + 1, tail);
                        }
                        assert!(ranges[i].end <= range_end);
                        if range_start <= ranges[i].start {
                            ranges[i].label_location.insert(label, loc);
                            continue;
                        }
                        // Cutting some of the range from the start.
                        let mut tail = ranges[i].clone();
                        ranges[i].end = range_start;
                        tail.start = range_start;
                        tail.label_location.insert(label, loc);
                        ranges.insert(i + 1, tail);
                    }
                }
            }
        }
    }

    fn remove_incomplete_ranges(&mut self) {
        // Ranges with not-enough labels are discarded.
        let processed_labels_len = self.processed_labels.len();
        self.ranges
            .retain(|r| r.label_location.len() == processed_labels_len);
    }
}
