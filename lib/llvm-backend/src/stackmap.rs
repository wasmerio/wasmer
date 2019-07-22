// https://llvm.org/docs/StackMaps.html#stackmap-section

use byteorder::{LittleEndian, ReadBytesExt};
use std::collections::HashMap;
use std::io::{self, Cursor};
use wasmer_runtime_core::state::{
    x64::{new_machine_state, X64Register, GPR},
    FunctionStateMap, MachineStateDiff, MachineValue, ModuleStateMap, OffsetInfo, RegisterIndex,
    SuspendOffset, WasmAbstractValue,
};

#[derive(Default, Debug, Clone)]
pub struct StackmapRegistry {
    pub entries: Vec<StackmapEntry>,
}

#[derive(Debug, Clone)]
pub struct StackmapEntry {
    pub kind: StackmapEntryKind,
    pub local_function_id: usize,
    pub opcode_offset: usize,
    pub value_semantics: Vec<ValueSemantic>,
    pub local_count: usize,
    pub stack_count: usize,
}

#[derive(Debug, Clone)]
pub enum ValueSemantic {
    WasmLocal(usize),
    WasmStack(usize),
}

#[derive(Debug, Clone, Copy)]
pub enum StackmapEntryKind {
    FunctionHeader,
    Loop,
    Call,
    Trappable,
}

/*
pub struct FunctionStateMap {
    pub initial: MachineState,
    pub local_function_id: usize,
    pub locals: Vec<WasmAbstractValue>,
    pub shadow_size: usize, // for single-pass backend, 32 bytes on x86-64
    pub diffs: Vec<MachineStateDiff>,
    pub wasm_function_header_target_offset: Option<SuspendOffset>,
    pub wasm_offset_to_target_offset: BTreeMap<usize, SuspendOffset>,
    pub loop_offsets: BTreeMap<usize, OffsetInfo>, /* suspend_offset -> info */
    pub call_offsets: BTreeMap<usize, OffsetInfo>, /* suspend_offset -> info */
    pub trappable_offsets: BTreeMap<usize, OffsetInfo>, /* suspend_offset -> info */
}
pub struct MachineStateDiff {
    pub last: Option<usize>,
    pub stack_push: Vec<MachineValue>,
    pub stack_pop: usize,
    pub reg_diff: Vec<(RegisterIndex, MachineValue)>,

    pub wasm_stack_push: Vec<WasmAbstractValue>,
    pub wasm_stack_pop: usize,
    pub wasm_stack_private_depth: usize, // absolute value; not a diff.

    pub wasm_inst_offset: usize, // absolute value; not a diff.
}
*/

impl StackmapEntry {
    pub fn populate_msm(
        &self,
        code_addr: usize,
        llvm_map: &StackMap,
        size_record: &StkSizeRecord,
        map_record: &StkMapRecord,
        msm: &mut ModuleStateMap,
    ) {
        #[derive(Copy, Clone, Debug)]
        enum RuntimeOrConstant {
            Runtime(MachineValue),
            Constant(u64),
        }

        let fsm = msm
            .local_functions
            .entry(self.local_function_id)
            .or_insert_with(|| {
                FunctionStateMap::new(new_machine_state(), self.local_function_id, 0, vec![])
            });

        assert_eq!(self.value_semantics.len(), map_record.locations.len());
        assert!(size_record.stack_size % 8 == 0);

        let mut machine_stack_layout: Vec<MachineValue> =
            vec![MachineValue::Undefined; (size_record.stack_size as usize) / 8];
        let mut regs: Vec<(RegisterIndex, MachineValue)> = vec![];
        let mut stack_constants: HashMap<usize, u64> = HashMap::new();

        let mut wasm_locals: Vec<WasmAbstractValue> = vec![];
        let mut wasm_stack: Vec<WasmAbstractValue> = vec![];

        for (i, loc) in map_record.locations.iter().enumerate() {
            let mv = match self.value_semantics[i] {
                ValueSemantic::WasmLocal(x) => {
                    if x != wasm_locals.len() {
                        panic!("unordered local values");
                    }
                    wasm_locals.push(WasmAbstractValue::Runtime);
                    MachineValue::WasmLocal(x)
                }
                ValueSemantic::WasmStack(x) => {
                    if x != wasm_stack.len() {
                        panic!("unordered stack values");
                    }
                    wasm_stack.push(WasmAbstractValue::Runtime);
                    MachineValue::WasmStack(x)
                }
            };
            match loc.ty {
                LocationType::Register => {
                    let index = X64Register::from_dwarf_regnum(loc.dwarf_regnum)
                        .expect("invalid regnum")
                        .to_index();
                    regs.push((index, mv));
                }
                LocationType::Constant => {
                    let v = loc.offset_or_small_constant as u32 as u64;
                    match mv {
                        MachineValue::WasmStack(x) => {
                            stack_constants.insert(x, v);
                            *wasm_stack.last_mut().unwrap() = WasmAbstractValue::Const(v);
                        }
                        _ => {} // TODO
                    }
                }
                LocationType::ConstantIndex => {
                    let v =
                        llvm_map.constants[loc.offset_or_small_constant as usize].large_constant;
                    match mv {
                        MachineValue::WasmStack(x) => {
                            stack_constants.insert(x, v);
                            *wasm_stack.last_mut().unwrap() = WasmAbstractValue::Const(v);
                        }
                        _ => {} // TODO
                    }
                }
                LocationType::Direct => match mv {
                    MachineValue::WasmLocal(_) => {
                        assert_eq!(loc.location_size, 8);
                        assert!(loc.offset_or_small_constant < 0);
                        assert!(
                            X64Register::from_dwarf_regnum(loc.dwarf_regnum).unwrap()
                                == X64Register::GPR(GPR::RBP)
                        );
                        let stack_offset = ((-loc.offset_or_small_constant) % 8) as usize;
                        assert!(stack_offset > 0 && stack_offset <= machine_stack_layout.len());
                        machine_stack_layout[stack_offset - 1] = mv;
                    }
                    _ => unreachable!(
                        "Direct location type is not expected for values other than local"
                    ),
                },
                LocationType::Indirect => {
                    assert_eq!(loc.location_size, 8);
                    assert!(loc.offset_or_small_constant < 0);
                    assert!(
                        X64Register::from_dwarf_regnum(loc.dwarf_regnum).unwrap()
                            == X64Register::GPR(GPR::RBP)
                    );
                    let stack_offset = ((-loc.offset_or_small_constant) % 8) as usize;
                    assert!(stack_offset > 0 && stack_offset <= machine_stack_layout.len());
                    machine_stack_layout[stack_offset - 1] = mv;
                }
            }
        }

        assert_eq!(wasm_stack.len(), self.stack_count);
        assert_eq!(wasm_locals.len(), self.local_count);

        let diff = MachineStateDiff {
            last: None,
            stack_push: machine_stack_layout,
            stack_pop: 0,
            reg_diff: regs,
            wasm_stack_push: wasm_stack,
            wasm_stack_pop: 0,
            wasm_stack_private_depth: 0,
            wasm_inst_offset: self.opcode_offset,
        };
        let diff_id = fsm.diffs.len();
        fsm.diffs.push(diff);

        match self.kind {
            StackmapEntryKind::FunctionHeader => {
                fsm.locals = wasm_locals;
            }
            _ => {
                assert_eq!(fsm.locals, wasm_locals);
            }
        }
        let target_offset = (size_record.function_address as usize)
            .checked_sub(code_addr)
            .unwrap()
            + map_record.instruction_offset as usize;

        match self.kind {
            StackmapEntryKind::Loop => {
                fsm.wasm_offset_to_target_offset
                    .insert(self.opcode_offset, SuspendOffset::Loop(target_offset));
                fsm.loop_offsets.insert(
                    target_offset,
                    OffsetInfo {
                        diff_id,
                        activate_offset: target_offset,
                    },
                );
            }
            StackmapEntryKind::Call => {
                fsm.wasm_offset_to_target_offset
                    .insert(self.opcode_offset, SuspendOffset::Call(target_offset));
                fsm.call_offsets.insert(
                    target_offset,
                    OffsetInfo {
                        diff_id,
                        activate_offset: target_offset,
                    },
                );
            }
            StackmapEntryKind::Trappable => {
                fsm.wasm_offset_to_target_offset
                    .insert(self.opcode_offset, SuspendOffset::Trappable(target_offset));
                fsm.trappable_offsets.insert(
                    target_offset,
                    OffsetInfo {
                        diff_id,
                        activate_offset: target_offset,
                    },
                );
            }
            StackmapEntryKind::FunctionHeader => {
                fsm.wasm_function_header_target_offset = Some(SuspendOffset::Loop(target_offset));
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StackMap {
    pub version: u8,
    pub stk_size_records: Vec<StkSizeRecord>,
    pub constants: Vec<Constant>,
    pub stk_map_records: Vec<StkMapRecord>,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct StkSizeRecord {
    pub function_address: u64,
    pub stack_size: u64,
    pub record_count: u64,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Constant {
    pub large_constant: u64,
}

#[derive(Clone, Debug, Default)]
pub struct StkMapRecord {
    pub patchpoint_id: u64,
    pub instruction_offset: u32,
    pub locations: Vec<Location>,
    pub live_outs: Vec<LiveOut>,
}

#[derive(Copy, Clone, Debug)]
pub struct Location {
    pub ty: LocationType,
    pub location_size: u16,
    pub dwarf_regnum: u16,
    pub offset_or_small_constant: i32,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct LiveOut {
    pub dwarf_regnum: u16,
    pub size_in_bytes: u8,
}

#[derive(Copy, Clone, Debug)]
pub enum LocationType {
    Register,
    Direct,
    Indirect,
    Constant,
    ConstantIndex,
}

impl StackMap {
    pub fn parse(raw: &[u8]) -> io::Result<StackMap> {
        let mut reader = Cursor::new(raw);
        let mut map = StackMap::default();

        let version = reader.read_u8()?;
        if version != 3 {
            return Err(io::Error::new(io::ErrorKind::Other, "version is not 3"));
        }
        map.version = version;
        if reader.read_u8()? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "reserved field is not zero (1)",
            ));
        }
        if reader.read_u16::<LittleEndian>()? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "reserved field is not zero (2)",
            ));
        }
        let num_functions = reader.read_u32::<LittleEndian>()?;
        let num_constants = reader.read_u32::<LittleEndian>()?;
        let num_records = reader.read_u32::<LittleEndian>()?;
        for _ in 0..num_functions {
            let mut record = StkSizeRecord::default();
            record.function_address = reader.read_u64::<LittleEndian>()?;
            record.stack_size = reader.read_u64::<LittleEndian>()?;
            record.record_count = reader.read_u64::<LittleEndian>()?;
            map.stk_size_records.push(record);
        }
        for _ in 0..num_constants {
            map.constants.push(Constant {
                large_constant: reader.read_u64::<LittleEndian>()?,
            });
        }
        for _ in 0..num_records {
            let mut record = StkMapRecord::default();

            record.patchpoint_id = reader.read_u64::<LittleEndian>()?;
            record.instruction_offset = reader.read_u32::<LittleEndian>()?;
            if reader.read_u16::<LittleEndian>()? != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "reserved field is not zero (3)",
                ));
            }
            let num_locations = reader.read_u16::<LittleEndian>()?;
            for _ in 0..num_locations {
                let ty = reader.read_u8()?;

                let mut location = Location {
                    ty: match ty {
                        1 => LocationType::Register,
                        2 => LocationType::Direct,
                        3 => LocationType::Indirect,
                        4 => LocationType::Constant,
                        5 => LocationType::ConstantIndex,
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::Other,
                                "unknown location type",
                            ))
                        }
                    },
                    location_size: 0,
                    dwarf_regnum: 0,
                    offset_or_small_constant: 0,
                };

                if reader.read_u8()? != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "reserved field is not zero (4)",
                    ));
                }
                location.location_size = reader.read_u16::<LittleEndian>()?;
                location.dwarf_regnum = reader.read_u16::<LittleEndian>()?;
                if reader.read_u16::<LittleEndian>()? != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "reserved field is not zero (5)",
                    ));
                }
                location.offset_or_small_constant = reader.read_i32::<LittleEndian>()?;

                record.locations.push(location);
            }
            if reader.position() % 8 != 0 {
                if reader.read_u32::<LittleEndian>()? != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "reserved field is not zero (6)",
                    ));
                }
            }
            if reader.read_u16::<LittleEndian>()? != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "reserved field is not zero (7)",
                ));
            }
            let num_live_outs = reader.read_u16::<LittleEndian>()?;
            for _ in 0..num_live_outs {
                let mut liveout = LiveOut::default();

                liveout.dwarf_regnum = reader.read_u16::<LittleEndian>()?;
                if reader.read_u8()? != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "reserved field is not zero (8)",
                    ));
                }
                liveout.size_in_bytes = reader.read_u8()?;

                record.live_outs.push(liveout);
            }
            if reader.position() % 8 != 0 {
                if reader.read_u32::<LittleEndian>()? != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "reserved field is not zero (9)",
                    ));
                }
            }

            map.stk_map_records.push(record);
        }
        Ok(map)
    }
}
