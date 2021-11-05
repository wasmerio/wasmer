// https://llvm.org/docs/StackMaps.html#stackmap-section

use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{self, Cursor};
use wasmer_vm_core::vm::Ctx;
use wasmer_vm_core::{
    module::Module,
    structures::TypedIndex,
    types::{GlobalIndex, LocalOrImport, TableIndex},
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
    pub is_start: bool,
}

#[derive(Debug, Clone)]
pub enum ValueSemantic {
    WasmLocal(usize),
    WasmStack(usize),
    Ctx,
    SignalMem,
    PointerToMemoryBase,
    PointerToMemoryBound, // 64-bit
    MemoryBase,
    MemoryBound, // 64-bit
    PointerToGlobal(usize),
    Global(usize),
    PointerToTableBase,
    PointerToTableBound,
    ImportedFuncPointer(usize),
    ImportedFuncCtx(usize),
    DynamicSigindice(usize),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StackmapEntryKind {
    FunctionHeader,
    Loop,
    Call,
    Trappable,
}

impl StackmapEntry {
    #[cfg(all(
        any(target_os = "freebsd", target_os = "linux", target_vendor = "apple"),
        target_arch = "x86_64"
    ))]
    pub fn populate_msm(
        &self,
        module_info: &ModuleInfo,
        code_addr: usize,
        llvm_map: &StackMap,
        size_record: &StkSizeRecord,
        map_record: &StkMapRecord,
        end: Option<(&StackmapEntry, &StkMapRecord)>,
        msm: &mut wasmer_vm_core::state::ModuleStateMap,
    ) {
        use std::collections::{BTreeMap, HashMap};
        use wasmer_vm_core::state::{
            x64::{new_machine_state, X64Register, GPR},
            FunctionStateMap, MachineStateDiff, MachineValue, OffsetInfo, RegisterIndex,
            SuspendOffset, WasmAbstractValue,
        };
        use wasmer_vm_core::vm;

        let func_base_addr = (size_record.function_address as usize)
            .checked_sub(code_addr)
            .unwrap();
        let target_offset = func_base_addr + map_record.instruction_offset as usize;
        assert!(self.is_start);

        if msm.local_functions.len() == self.local_function_id {
            assert_eq!(self.kind, StackmapEntryKind::FunctionHeader);
            msm.local_functions.insert(
                target_offset,
                FunctionStateMap::new(new_machine_state(), self.local_function_id, 0, vec![]),
            );
        } else if msm.local_functions.len() == self.local_function_id + 1 {
        } else {
            panic!("unordered local functions");
        }

        let (_, fsm) = msm.local_functions.iter_mut().last().unwrap();

        assert_eq!(self.value_semantics.len(), map_record.locations.len());

        // System V requires 16-byte alignment before each call instruction.
        // Considering the saved rbp we need to ensure the stack size % 16 always equals to 8.
        assert!(size_record.stack_size % 16 == 8);

        // Layout begins just below saved rbp. (push rbp; mov rbp, rsp)
        let mut machine_stack_half_layout: Vec<MachineValue> =
            vec![MachineValue::Undefined; (size_record.stack_size - 8) as usize / 4];
        let mut regs: Vec<(RegisterIndex, MachineValue)> = vec![];
        let mut stack_constants: HashMap<usize, u64> = HashMap::new();

        let mut prev_frame_diff: BTreeMap<usize, Option<MachineValue>> = BTreeMap::new();

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
                ValueSemantic::Ctx => MachineValue::Vmctx,
                ValueSemantic::SignalMem => {
                    MachineValue::VmctxDeref(vec![Ctx::offset_interrupt_signal_mem() as usize, 0])
                }
                ValueSemantic::PointerToMemoryBase => {
                    MachineValue::VmctxDeref(vec![Ctx::offset_memory_base() as usize])
                }
                ValueSemantic::PointerToMemoryBound => {
                    MachineValue::VmctxDeref(vec![Ctx::offset_memory_bound() as usize])
                }
                ValueSemantic::MemoryBase => {
                    MachineValue::VmctxDeref(vec![Ctx::offset_memory_base() as usize, 0])
                }
                ValueSemantic::MemoryBound => {
                    MachineValue::VmctxDeref(vec![Ctx::offset_memory_bound() as usize, 0])
                }
                ValueSemantic::PointerToGlobal(idx) => {
                    MachineValue::VmctxDeref(deref_global(module_info, idx, false))
                }
                ValueSemantic::Global(idx) => {
                    MachineValue::VmctxDeref(deref_global(module_info, idx, true))
                }
                ValueSemantic::PointerToTableBase => {
                    MachineValue::VmctxDeref(deref_table_base(module_info, 0, false))
                }
                ValueSemantic::PointerToTableBound => {
                    MachineValue::VmctxDeref(deref_table_bound(module_info, 0, false))
                }
                ValueSemantic::ImportedFuncPointer(idx) => MachineValue::VmctxDeref(vec![
                    Ctx::offset_imported_funcs() as usize,
                    vm::ImportedFunc::size() as usize * idx
                        + vm::ImportedFunc::offset_func() as usize,
                    0,
                ]),
                ValueSemantic::ImportedFuncCtx(idx) => MachineValue::VmctxDeref(vec![
                    Ctx::offset_imported_funcs() as usize,
                    vm::ImportedFunc::size() as usize * idx
                        + vm::ImportedFunc::offset_func_ctx() as usize,
                    0,
                ]),
                ValueSemantic::DynamicSigindice(idx) => {
                    MachineValue::VmctxDeref(vec![Ctx::offset_signatures() as usize, idx * 4, 0])
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
                        assert_eq!(loc.location_size, 8); // the pointer itself
                        assert!(
                            X64Register::from_dwarf_regnum(loc.dwarf_regnum).unwrap()
                                == X64Register::GPR(GPR::RBP)
                        );
                        if loc.offset_or_small_constant >= 0 {
                            assert!(loc.offset_or_small_constant >= 16); // (saved_rbp, return_address)
                            assert!(loc.offset_or_small_constant % 8 == 0);
                            prev_frame_diff
                                .insert((loc.offset_or_small_constant as usize - 16) / 8, Some(mv));
                        } else {
                            let stack_offset = ((-loc.offset_or_small_constant) / 4) as usize;
                            assert!(
                                stack_offset > 0 && stack_offset <= machine_stack_half_layout.len()
                            );
                            machine_stack_half_layout[stack_offset - 1] = mv;
                        }
                    }
                    _ => unreachable!(
                        "Direct location type is not expected for values other than local"
                    ),
                },
                LocationType::Indirect => {
                    assert!(loc.offset_or_small_constant < 0);
                    assert!(
                        X64Register::from_dwarf_regnum(loc.dwarf_regnum).unwrap()
                            == X64Register::GPR(GPR::RBP)
                    );
                    let stack_offset = ((-loc.offset_or_small_constant) / 4) as usize;
                    assert!(stack_offset > 0 && stack_offset <= machine_stack_half_layout.len());
                    machine_stack_half_layout[stack_offset - 1] = mv;
                }
            }
        }

        assert_eq!(wasm_stack.len(), self.stack_count);
        assert_eq!(wasm_locals.len(), self.local_count);

        let mut machine_stack_layout: Vec<MachineValue> =
            Vec::with_capacity(machine_stack_half_layout.len() / 2);

        for i in 0..machine_stack_half_layout.len() / 2 {
            let major = &machine_stack_half_layout[i * 2 + 1]; // mod 8 == 0
            let minor = &machine_stack_half_layout[i * 2]; // mod 8 == 4
            let only_major = match *minor {
                MachineValue::Undefined => true,
                _ => false,
            };
            if only_major {
                machine_stack_layout.push(major.clone());
            } else {
                machine_stack_layout.push(MachineValue::TwoHalves(Box::new((
                    major.clone(),
                    minor.clone(),
                ))));
            }
        }

        let diff = MachineStateDiff {
            last: None,
            stack_push: machine_stack_layout,
            stack_pop: 0,
            prev_frame_diff,
            reg_diff: regs,
            wasm_stack_push: wasm_stack,
            wasm_stack_pop: 0,
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

        let end_offset = {
            if let Some(end) = end {
                let (end_entry, end_record) = end;
                assert_eq!(end_entry.is_start, false);
                assert_eq!(self.opcode_offset, end_entry.opcode_offset);
                let end_offset = func_base_addr + end_record.instruction_offset as usize;
                assert!(end_offset >= target_offset);
                end_offset
            } else {
                target_offset + 1
            }
        };

        match self.kind {
            StackmapEntryKind::Loop => {
                fsm.wasm_offset_to_target_offset
                    .insert(self.opcode_offset, SuspendOffset::Loop(target_offset));
                fsm.loop_offsets.insert(
                    target_offset,
                    OffsetInfo {
                        end_offset,
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
                        end_offset: end_offset + 1, // The return address is just after 'call' instruction. Offset by one here.
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
                        end_offset,
                        diff_id,
                        activate_offset: target_offset,
                    },
                );
            }
            StackmapEntryKind::FunctionHeader => {
                fsm.wasm_function_header_target_offset = Some(SuspendOffset::Loop(target_offset));
                fsm.loop_offsets.insert(
                    target_offset,
                    OffsetInfo {
                        end_offset,
                        diff_id,
                        activate_offset: target_offset,
                    },
                );
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

fn deref_global(info: &ModuleInfo, idx: usize, deref_into_value: bool) -> Vec<usize> {
    let mut x: Vec<usize> = match GlobalIndex::new(idx).local_or_import(info) {
        LocalOrImport::Local(idx) => vec![Ctx::offset_globals() as usize, idx.index() * 8, 0],
        LocalOrImport::Import(idx) => {
            vec![Ctx::offset_imported_globals() as usize, idx.index() * 8, 0]
        }
    };
    if deref_into_value {
        x.push(0);
    }
    x
}

fn deref_table_base(info: &ModuleInfo, idx: usize, deref_into_value: bool) -> Vec<usize> {
    let mut x: Vec<usize> = match TableIndex::new(idx).local_or_import(info) {
        LocalOrImport::Local(idx) => vec![Ctx::offset_tables() as usize, idx.index() * 8, 0],
        LocalOrImport::Import(idx) => {
            vec![Ctx::offset_imported_tables() as usize, idx.index() * 8, 0]
        }
    };
    if deref_into_value {
        x.push(0);
    }
    x
}

fn deref_table_bound(info: &ModuleInfo, idx: usize, deref_into_value: bool) -> Vec<usize> {
    let mut x: Vec<usize> = match TableIndex::new(idx).local_or_import(info) {
        LocalOrImport::Local(idx) => vec![Ctx::offset_tables() as usize, idx.index() * 8, 8],
        LocalOrImport::Import(idx) => {
            vec![Ctx::offset_imported_tables() as usize, idx.index() * 8, 8]
        }
    };
    if deref_into_value {
        x.push(0);
    }
    x
}
