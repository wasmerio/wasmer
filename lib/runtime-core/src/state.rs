use std::collections::BTreeMap;
use std::ops::Bound::{Included, Unbounded};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct RegisterIndex(pub usize);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum WasmAbstractValue {
    Runtime,
    Const(u64),
}

#[derive(Clone, Debug)]
pub struct MachineState {
    pub stack_values: Vec<MachineValue>,
    pub register_values: Vec<MachineValue>,

    pub wasm_stack: Vec<WasmAbstractValue>,
    pub wasm_stack_private_depth: usize,

    pub wasm_inst_offset: usize,
}

#[derive(Clone, Debug, Default)]
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MachineValue {
    Undefined,
    Vmctx,
    PreserveRegister(RegisterIndex),
    CopyStackBPRelative(i32), // relative to Base Pointer, in byte offset
    ExplicitShadow,           // indicates that all values above this are above the shadow region
    WasmStack(usize),
    WasmLocal(usize),
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Copy, Debug)]
pub enum SuspendOffset {
    Loop(usize),
    Call(usize),
    Trappable(usize),
}

#[derive(Clone, Debug)]
pub struct OffsetInfo {
    pub diff_id: usize,
    pub activate_offset: usize,
}

#[derive(Clone, Debug)]
pub struct ModuleStateMap {
    pub local_functions: BTreeMap<usize, FunctionStateMap>,
    pub total_size: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WasmFunctionStateDump {
    pub local_function_id: usize,
    pub wasm_inst_offset: usize,
    pub stack: Vec<Option<u64>>,
    pub locals: Vec<Option<u64>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionStateImage {
    pub frames: Vec<WasmFunctionStateDump>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceImage {
    pub memory: Option<Vec<u8>>,
    pub globals: Vec<u64>,
    pub execution_state: ExecutionStateImage,
}

impl ModuleStateMap {
    fn lookup_call_ip(&self, ip: usize, base: usize) -> Option<(&FunctionStateMap, MachineState)> {
        if ip < base || ip - base >= self.total_size {
            None
        } else {
            let (_, fsm) = self
                .local_functions
                .range((Unbounded, Included(&(ip - base))))
                .last()
                .unwrap();

            match fsm.call_offsets.get(&(ip - base)) {
                Some(x) => {
                    if x.diff_id < fsm.diffs.len() {
                        Some((fsm, fsm.diffs[x.diff_id].build_state(fsm)))
                    } else {
                        None
                    }
                }
                None => None,
            }
        }
    }

    fn lookup_trappable_ip(
        &self,
        ip: usize,
        base: usize,
    ) -> Option<(&FunctionStateMap, MachineState)> {
        if ip < base || ip - base >= self.total_size {
            None
        } else {
            let (_, fsm) = self
                .local_functions
                .range((Unbounded, Included(&(ip - base))))
                .last()
                .unwrap();

            match fsm.trappable_offsets.get(&(ip - base)) {
                Some(x) => {
                    if x.diff_id < fsm.diffs.len() {
                        Some((fsm, fsm.diffs[x.diff_id].build_state(fsm)))
                    } else {
                        None
                    }
                }
                None => None,
            }
        }
    }

    fn lookup_loop_ip(&self, ip: usize, base: usize) -> Option<(&FunctionStateMap, MachineState)> {
        if ip < base || ip - base >= self.total_size {
            None
        } else {
            let (_, fsm) = self
                .local_functions
                .range((Unbounded, Included(&(ip - base))))
                .last()
                .unwrap();

            match fsm.loop_offsets.get(&(ip - base)) {
                Some(x) => {
                    if x.diff_id < fsm.diffs.len() {
                        Some((fsm, fsm.diffs[x.diff_id].build_state(fsm)))
                    } else {
                        None
                    }
                }
                None => None,
            }
        }
    }
}

impl FunctionStateMap {
    pub fn new(
        initial: MachineState,
        local_function_id: usize,
        shadow_size: usize,
        locals: Vec<WasmAbstractValue>,
    ) -> FunctionStateMap {
        FunctionStateMap {
            initial,
            local_function_id,
            shadow_size,
            locals,
            diffs: vec![],
            wasm_function_header_target_offset: None,
            wasm_offset_to_target_offset: BTreeMap::new(),
            loop_offsets: BTreeMap::new(),
            call_offsets: BTreeMap::new(),
            trappable_offsets: BTreeMap::new(),
        }
    }
}

impl MachineState {
    pub fn diff(&self, old: &MachineState) -> MachineStateDiff {
        let first_diff_stack_depth: usize = self
            .stack_values
            .iter()
            .zip(old.stack_values.iter())
            .enumerate()
            .find(|&(_, (&a, &b))| a != b)
            .map(|x| x.0)
            .unwrap_or(old.stack_values.len().min(self.stack_values.len()));
        assert_eq!(self.register_values.len(), old.register_values.len());
        let reg_diff: Vec<_> = self
            .register_values
            .iter()
            .zip(old.register_values.iter())
            .enumerate()
            .filter(|&(_, (&a, &b))| a != b)
            .map(|(i, (&a, _))| (RegisterIndex(i), a))
            .collect();
        let first_diff_wasm_stack_depth: usize = self
            .wasm_stack
            .iter()
            .zip(old.wasm_stack.iter())
            .enumerate()
            .find(|&(_, (&a, &b))| a != b)
            .map(|x| x.0)
            .unwrap_or(old.wasm_stack.len().min(self.wasm_stack.len()));
        MachineStateDiff {
            last: None,
            stack_push: self.stack_values[first_diff_stack_depth..].to_vec(),
            stack_pop: old.stack_values.len() - first_diff_stack_depth,
            reg_diff: reg_diff,

            wasm_stack_push: self.wasm_stack[first_diff_wasm_stack_depth..].to_vec(),
            wasm_stack_pop: old.wasm_stack.len() - first_diff_wasm_stack_depth,
            wasm_stack_private_depth: self.wasm_stack_private_depth,

            wasm_inst_offset: self.wasm_inst_offset,
        }
    }
}

impl MachineStateDiff {
    pub fn build_state(&self, m: &FunctionStateMap) -> MachineState {
        let mut chain: Vec<&MachineStateDiff> = vec![];
        chain.push(self);
        let mut current = self.last;
        while let Some(x) = current {
            let that = &m.diffs[x];
            current = that.last;
            chain.push(that);
        }
        chain.reverse();
        let mut state = m.initial.clone();
        for x in chain {
            for _ in 0..x.stack_pop {
                state.stack_values.pop().unwrap();
            }
            for v in &x.stack_push {
                state.stack_values.push(*v);
            }
            for &(index, v) in &x.reg_diff {
                state.register_values[index.0] = v;
            }
            for _ in 0..x.wasm_stack_pop {
                state.wasm_stack.pop().unwrap();
            }
            for v in &x.wasm_stack_push {
                state.wasm_stack.push(*v);
            }
        }
        state.wasm_stack_private_depth = self.wasm_stack_private_depth;
        state.wasm_inst_offset = self.wasm_inst_offset;
        state
    }
}

impl ExecutionStateImage {
    pub fn print_backtrace_if_needed(&self) {
        use std::env;

        if let Ok(x) = env::var("WASMER_BACKTRACE") {
            if x == "1" {
                eprintln!("{}", self.colored_output());
                return;
            }
        }

        eprintln!("Run with `WASMER_BACKTRACE=1` environment variable to display a backtrace.");
    }

    pub fn colored_output(&self) -> String {
        use colored::*;

        fn join_strings(x: impl Iterator<Item = String>, sep: &str) -> String {
            let mut ret = String::new();
            let mut first = true;

            for s in x {
                if first {
                    first = false;
                } else {
                    ret += sep;
                }
                ret += &s;
            }

            ret
        }

        fn format_optional_u64_sequence(x: &[Option<u64>]) -> String {
            if x.len() == 0 {
                "(empty)".into()
            } else {
                join_strings(
                    x.iter().enumerate().map(|(i, x)| {
                        format!(
                            "[{}] = {}",
                            i,
                            x.map(|x| format!("{}", x))
                                .unwrap_or_else(|| "?".to_string())
                                .bold()
                                .cyan()
                        )
                    }),
                    ", ",
                )
            }
        }

        let mut ret = String::new();

        if self.frames.len() == 0 {
            ret += &"Unknown fault address, cannot read stack.".yellow();
            ret += "\n";
        } else {
            ret += &"Backtrace:".bold();
            ret += "\n";
            for (i, f) in self.frames.iter().enumerate() {
                ret += &format!("* Frame {} @ Local function {}", i, f.local_function_id).bold();
                ret += "\n";
                ret += &format!(
                    "  {} {}\n",
                    "Offset:".bold().yellow(),
                    format!("{}", f.wasm_inst_offset).bold().cyan(),
                );
                ret += &format!(
                    "  {} {}\n",
                    "Locals:".bold().yellow(),
                    format_optional_u64_sequence(&f.locals)
                );
                ret += &format!(
                    "  {} {}\n\n",
                    "Stack:".bold().yellow(),
                    format_optional_u64_sequence(&f.stack)
                );
            }
        }

        ret
    }
}

impl InstanceImage {
    pub fn from_bytes(input: &[u8]) -> Option<InstanceImage> {
        use bincode::deserialize;
        match deserialize(input) {
            Ok(x) => Some(x),
            Err(_) => None,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        use bincode::serialize;
        serialize(self).unwrap()
    }
}

#[cfg(all(unix, target_arch = "x86_64"))]
pub mod x64 {
    use super::*;
    use crate::codegen::BreakpointMap;
    use crate::fault::{catch_unsafe_unwind, run_on_alternative_stack};
    use crate::structures::TypedIndex;
    use crate::types::LocalGlobalIndex;
    use crate::vm::Ctx;
    use std::any::Any;

    pub fn new_machine_state() -> MachineState {
        MachineState {
            stack_values: vec![],
            register_values: vec![MachineValue::Undefined; 16 + 8],
            wasm_stack: vec![],
            wasm_stack_private_depth: 0,
            wasm_inst_offset: ::std::usize::MAX,
        }
    }

    #[warn(unused_variables)]
    pub unsafe fn invoke_call_return_on_stack(
        msm: &ModuleStateMap,
        code_base: usize,
        image: InstanceImage,
        vmctx: &mut Ctx,
        breakpoints: Option<BreakpointMap>,
    ) -> Result<u64, Box<dyn Any>> {
        let mut stack: Vec<u64> = vec![0; 1048576 * 8 / 8]; // 8MB stack
        let mut stack_offset: usize = stack.len();

        stack_offset -= 3; // placeholder for call return

        let mut last_stack_offset: u64 = 0; // rbp

        let mut known_registers: [Option<u64>; 24] = [None; 24];

        let local_functions_vec: Vec<&FunctionStateMap> =
            msm.local_functions.iter().map(|(_, v)| v).collect();

        // Bottom to top
        for f in image.execution_state.frames.iter().rev() {
            let fsm = local_functions_vec[f.local_function_id];
            let suspend_offset = if f.wasm_inst_offset == ::std::usize::MAX {
                fsm.wasm_function_header_target_offset
            } else {
                fsm.wasm_offset_to_target_offset
                    .get(&f.wasm_inst_offset)
                    .map(|x| *x)
            }
            .expect("instruction is not a critical point");

            let (activate_offset, diff_id) = match suspend_offset {
                SuspendOffset::Loop(x) => fsm.loop_offsets.get(&x),
                SuspendOffset::Call(x) => fsm.call_offsets.get(&x),
                SuspendOffset::Trappable(x) => fsm.trappable_offsets.get(&x),
            }
            .map(|x| (x.activate_offset, x.diff_id))
            .expect("offset cannot be found in table");

            let diff = &fsm.diffs[diff_id];
            let state = diff.build_state(fsm);

            stack_offset -= 1;
            stack[stack_offset] = stack.as_ptr().offset(last_stack_offset as isize) as usize as u64; // push rbp
            last_stack_offset = stack_offset as _;

            let mut got_explicit_shadow = false;

            for v in state.stack_values.iter() {
                match *v {
                    MachineValue::Undefined => stack_offset -= 1,
                    MachineValue::Vmctx => {
                        stack_offset -= 1;
                        stack[stack_offset] = vmctx as *mut Ctx as usize as u64;
                    }
                    MachineValue::PreserveRegister(index) => {
                        stack_offset -= 1;
                        stack[stack_offset] = known_registers[index.0].unwrap_or(0);
                    }
                    MachineValue::CopyStackBPRelative(byte_offset) => {
                        assert!(byte_offset % 8 == 0);
                        let target_offset = (byte_offset / 8) as isize;
                        let v = stack[(last_stack_offset as isize + target_offset) as usize];
                        stack_offset -= 1;
                        stack[stack_offset] = v;
                    }
                    MachineValue::ExplicitShadow => {
                        assert!(fsm.shadow_size % 8 == 0);
                        stack_offset -= fsm.shadow_size / 8;
                        got_explicit_shadow = true;
                    }
                    MachineValue::WasmStack(x) => {
                        stack_offset -= 1;
                        match state.wasm_stack[x] {
                            WasmAbstractValue::Const(x) => {
                                stack[stack_offset] = x;
                            }
                            WasmAbstractValue::Runtime => {
                                stack[stack_offset] = f.stack[x].unwrap();
                            }
                        }
                    }
                    MachineValue::WasmLocal(x) => {
                        stack_offset -= 1;
                        match fsm.locals[x] {
                            WasmAbstractValue::Const(x) => {
                                stack[stack_offset] = x;
                            }
                            WasmAbstractValue::Runtime => {
                                stack[stack_offset] = f.locals[x].unwrap();
                            }
                        }
                    }
                }
            }
            if !got_explicit_shadow {
                assert!(fsm.shadow_size % 8 == 0);
                stack_offset -= fsm.shadow_size / 8;
            }
            for (i, v) in state.register_values.iter().enumerate() {
                match *v {
                    MachineValue::Undefined => {}
                    MachineValue::Vmctx => {
                        known_registers[i] = Some(vmctx as *mut Ctx as usize as u64);
                    }
                    MachineValue::WasmStack(x) => match state.wasm_stack[x] {
                        WasmAbstractValue::Const(x) => {
                            known_registers[i] = Some(x);
                        }
                        WasmAbstractValue::Runtime => {
                            known_registers[i] = Some(f.stack[x].unwrap());
                        }
                    },
                    MachineValue::WasmLocal(x) => match fsm.locals[x] {
                        WasmAbstractValue::Const(x) => {
                            known_registers[i] = Some(x);
                        }
                        WasmAbstractValue::Runtime => {
                            known_registers[i] = Some(f.locals[x].unwrap());
                        }
                    },
                    _ => unreachable!(),
                }
            }

            // no need to check 16-byte alignment here because it's possible that we're not at a call entry.

            stack_offset -= 1;
            stack[stack_offset] = (code_base + activate_offset) as u64; // return address
        }

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R15).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R14).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R13).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R12).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R11).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R10).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R9).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::R8).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::RSI).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::RDI).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::RDX).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::RCX).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::RBX).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = known_registers[X64Register::GPR(GPR::RAX).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] = stack.as_ptr().offset(last_stack_offset as isize) as usize as u64; // rbp

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM7).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM6).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM5).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM4).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM3).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM2).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM1).to_index().0].unwrap_or(0);

        stack_offset -= 1;
        stack[stack_offset] =
            known_registers[X64Register::XMM(XMM::XMM0).to_index().0].unwrap_or(0);

        if let Some(ref memory) = image.memory {
            assert!(vmctx.internal.memory_bound <= memory.len());

            if vmctx.internal.memory_bound < memory.len() {
                let grow: unsafe extern "C" fn(ctx: &mut Ctx, memory_index: usize, delta: usize) =
                    ::std::mem::transmute((*vmctx.internal.intrinsics).memory_grow);
                grow(
                    vmctx,
                    0,
                    (memory.len() - vmctx.internal.memory_bound) / 65536,
                );
                assert_eq!(vmctx.internal.memory_bound, memory.len());
            }

            ::std::slice::from_raw_parts_mut(
                vmctx.internal.memory_base,
                vmctx.internal.memory_bound,
            )
            .copy_from_slice(memory);
        }

        let globals_len = (*vmctx.module).info.globals.len();
        for i in 0..globals_len {
            (*(*vmctx.local_backing).globals[LocalGlobalIndex::new(i)].vm_local_global()).data =
                image.globals[i];
        }

        drop(image); // free up host memory

        catch_unsafe_unwind(
            || {
                run_on_alternative_stack(
                    stack.as_mut_ptr().offset(stack.len() as isize),
                    stack.as_mut_ptr().offset(stack_offset as isize),
                )
            },
            breakpoints,
        )
    }

    pub fn build_instance_image(
        vmctx: &mut Ctx,
        execution_state: ExecutionStateImage,
    ) -> InstanceImage {
        unsafe {
            let memory = if vmctx.internal.memory_base.is_null() {
                None
            } else {
                Some(
                    ::std::slice::from_raw_parts(
                        vmctx.internal.memory_base,
                        vmctx.internal.memory_bound,
                    )
                    .to_vec(),
                )
            };

            // FIXME: Imported globals
            let globals_len = (*vmctx.module).info.globals.len();
            let globals: Vec<u64> = (0..globals_len)
                .map(|i| {
                    (*vmctx.local_backing).globals[LocalGlobalIndex::new(i)]
                        .get()
                        .to_u64()
                })
                .collect();

            InstanceImage {
                memory: memory,
                globals: globals,
                execution_state: execution_state,
            }
        }
    }

    #[warn(unused_variables)]
    pub unsafe fn read_stack(
        msm: &ModuleStateMap,
        code_base: usize,
        mut stack: *const u64,
        initially_known_registers: [Option<u64>; 24],
        mut initial_address: Option<u64>,
    ) -> ExecutionStateImage {
        let mut known_registers: [Option<u64>; 24] = initially_known_registers;
        let mut results: Vec<WasmFunctionStateDump> = vec![];

        for _ in 0.. {
            let ret_addr = initial_address.take().unwrap_or_else(|| {
                let x = *stack;
                stack = stack.offset(1);
                x
            });
            let (fsm, state) = match msm
                .lookup_call_ip(ret_addr as usize, code_base)
                .or_else(|| msm.lookup_trappable_ip(ret_addr as usize, code_base))
                .or_else(|| msm.lookup_loop_ip(ret_addr as usize, code_base))
            {
                Some(x) => x,
                _ => return ExecutionStateImage { frames: results },
            };

            let mut wasm_stack: Vec<Option<u64>> = state
                .wasm_stack
                .iter()
                .map(|x| match *x {
                    WasmAbstractValue::Const(x) => Some(x),
                    WasmAbstractValue::Runtime => None,
                })
                .collect();
            let mut wasm_locals: Vec<Option<u64>> = fsm
                .locals
                .iter()
                .map(|x| match *x {
                    WasmAbstractValue::Const(x) => Some(x),
                    WasmAbstractValue::Runtime => None,
                })
                .collect();

            // This must be before the next loop because that modifies `known_registers`.
            for (i, v) in state.register_values.iter().enumerate() {
                match *v {
                    MachineValue::Undefined => {}
                    MachineValue::Vmctx => {}
                    MachineValue::WasmStack(idx) => {
                        if let Some(v) = known_registers[i] {
                            wasm_stack[idx] = Some(v);
                        } else {
                            eprintln!(
                                "BUG: Register {} for WebAssembly stack slot {} has unknown value.",
                                i, idx
                            );
                        }
                    }
                    MachineValue::WasmLocal(idx) => {
                        if let Some(v) = known_registers[i] {
                            wasm_locals[idx] = Some(v);
                        }
                    }
                    _ => unreachable!(),
                }
            }

            let mut found_shadow = false;
            for v in state.stack_values.iter() {
                match *v {
                    MachineValue::ExplicitShadow => {
                        found_shadow = true;
                        break;
                    }
                    _ => {}
                }
            }
            if !found_shadow {
                stack = stack.offset((fsm.shadow_size / 8) as isize);
            }

            for v in state.stack_values.iter().rev() {
                match *v {
                    MachineValue::ExplicitShadow => {
                        stack = stack.offset((fsm.shadow_size / 8) as isize);
                    }
                    MachineValue::Undefined => {
                        stack = stack.offset(1);
                    }
                    MachineValue::Vmctx => {
                        stack = stack.offset(1);
                    }
                    MachineValue::PreserveRegister(idx) => {
                        known_registers[idx.0] = Some(*stack);
                        stack = stack.offset(1);
                    }
                    MachineValue::CopyStackBPRelative(_) => {
                        stack = stack.offset(1);
                    }
                    MachineValue::WasmStack(idx) => {
                        wasm_stack[idx] = Some(*stack);
                        stack = stack.offset(1);
                    }
                    MachineValue::WasmLocal(idx) => {
                        wasm_locals[idx] = Some(*stack);
                        stack = stack.offset(1);
                    }
                }
            }
            stack = stack.offset(1); // RBP

            wasm_stack.truncate(
                wasm_stack
                    .len()
                    .checked_sub(state.wasm_stack_private_depth)
                    .unwrap(),
            );

            let wfs = WasmFunctionStateDump {
                local_function_id: fsm.local_function_id,
                wasm_inst_offset: state.wasm_inst_offset,
                stack: wasm_stack,
                locals: wasm_locals,
            };
            results.push(wfs);
        }

        unreachable!();
    }

    #[repr(u8)]
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub enum GPR {
        RAX,
        RCX,
        RDX,
        RBX,
        RSP,
        RBP,
        RSI,
        RDI,
        R8,
        R9,
        R10,
        R11,
        R12,
        R13,
        R14,
        R15,
    }

    #[repr(u8)]
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub enum XMM {
        XMM0,
        XMM1,
        XMM2,
        XMM3,
        XMM4,
        XMM5,
        XMM6,
        XMM7,
    }

    pub enum X64Register {
        GPR(GPR),
        XMM(XMM),
    }

    impl X64Register {
        pub fn to_index(&self) -> RegisterIndex {
            match *self {
                X64Register::GPR(x) => RegisterIndex(x as usize),
                X64Register::XMM(x) => RegisterIndex(x as usize + 16),
            }
        }
    }
}
