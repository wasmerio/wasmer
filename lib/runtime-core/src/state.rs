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
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MachineValue {
    Undefined,
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
    pub loop_offsets: BTreeMap<usize, usize>, /* offset -> diff_id */
    pub call_offsets: BTreeMap<usize, usize>, /* offset -> diff_id */
    pub trappable_offsets: BTreeMap<usize, usize>, /* offset -> diff_id */
}

#[derive(Clone, Debug)]
pub struct ModuleStateMap {
    pub local_functions: BTreeMap<usize, FunctionStateMap>,
    pub total_size: usize,
}

#[derive(Clone, Debug)]
pub struct WasmFunctionStateDump {
    pub local_function_id: usize,
    pub stack: Vec<Option<u64>>,
    pub locals: Vec<Option<u64>>,
}

impl ModuleStateMap {
    fn lookup_call_ip(
        &self,
        ip: usize,
        base: usize,
    ) -> Option<(&FunctionStateMap, MachineState)> {
        if ip < base || ip - base >= self.total_size {
            None
        } else {
            //println!("lookup ip: {} in {:?}", ip - base, self.local_functions);
            let (_, fsm) = self
                .local_functions
                .range((Unbounded, Included(&(ip - base))))
                .last()
                .unwrap();

            match fsm.call_offsets.get(&(ip - base)) {
                Some(x) => Some((fsm, fsm.diffs[*x].build_state(fsm))),
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
            //println!("lookup ip: {} in {:?}", ip - base, self.local_functions);
            let (_, fsm) = self
                .local_functions
                .range((Unbounded, Included(&(ip - base))))
                .last()
                .unwrap();

            match fsm.trappable_offsets.get(&(ip - base)) {
                Some(x) => Some((fsm, fsm.diffs[*x].build_state(fsm))),
                None => None,
            }
        }
    }
}

impl FunctionStateMap {
    pub fn new(initial: MachineState, local_function_id: usize, shadow_size: usize, locals: Vec<WasmAbstractValue>) -> FunctionStateMap {
        FunctionStateMap {
            initial,
            local_function_id,
            shadow_size,
            locals,
            diffs: vec![],
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
        state
    }
}

#[cfg(all(unix, target_arch = "x86_64"))]
pub mod x64 {
    use super::*;

    pub fn new_machine_state() -> MachineState {
        MachineState {
            stack_values: vec![],
            register_values: vec![MachineValue::Undefined; 16 + 8],
            wasm_stack: vec![],
            wasm_stack_private_depth: 0,
        }
    }

    #[warn(unused_variables)]
    pub unsafe fn read_stack(msm: &ModuleStateMap, code_base: usize, mut stack: *const u64, initially_known_registers: [Option<u64>; 24], mut initial_address: Option<u64>) -> Vec<WasmFunctionStateDump> {
        let mut known_registers: [Option<u64>; 24] = initially_known_registers;
        let mut results: Vec<WasmFunctionStateDump> = vec![];

        for _ in 0.. {
            let ret_addr = initial_address.take().unwrap_or_else(|| {
                let x = *stack;
                stack = stack.offset(1);
                x
            });
            let (fsm, state) = match
                msm.lookup_call_ip(ret_addr as usize, code_base)
                .or_else(|| msm.lookup_trappable_ip(ret_addr as usize, code_base))
            {
                Some(x) => x,
                _ => return results,
            };

            let mut wasm_stack: Vec<Option<u64>> = state.wasm_stack.iter()
                .map(|x| match *x {
                    WasmAbstractValue::Const(x) => Some(x),
                    WasmAbstractValue::Runtime => None,
                }).collect();
            let mut wasm_locals: Vec<Option<u64>> = fsm.locals.iter()
                .map(|x| match *x {
                    WasmAbstractValue::Const(x) => Some(x),
                    WasmAbstractValue::Runtime => None,
                }).collect();

            // This must be before the next loop because that modifies `known_registers`.
            for (i, v) in state.register_values.iter().enumerate() {
                match *v {
                    MachineValue::Undefined => {}
                    MachineValue::WasmStack(idx) => {
                        if let Some(v) = known_registers[i] {
                            wasm_stack[idx] = Some(v);
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
                    MachineValue::PreserveRegister(idx) => {
                        known_registers[idx.0] = Some(*stack);
                        stack = stack.offset(1);
                    }
                    MachineValue::CopyStackBPRelative(offset) => {
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

            wasm_stack.truncate(wasm_stack.len().checked_sub(state.wasm_stack_private_depth).unwrap());

            let wfs = WasmFunctionStateDump {
                local_function_id: fsm.local_function_id,
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
