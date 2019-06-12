use std::collections::BTreeMap;
use std::fmt::Debug;
use std::ops::Bound::{Included, Unbounded};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct RegisterIndex(pub usize);

#[derive(Clone, Debug)]
pub struct MachineState {
    pub stack_values: Vec<MachineValue>,
    pub register_values: Vec<MachineValue>,
}

#[derive(Clone, Debug, Default)]
pub struct MachineStateDiff {
    pub last: Option<usize>,
    pub stack_push: Vec<MachineValue>,
    pub stack_pop: usize,
    pub reg_diff: Vec<(RegisterIndex, MachineValue)>,
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
    pub shadow_size: usize, // for single-pass backend, 32 bytes on x86-64
    pub diffs: Vec<MachineStateDiff>,
    pub loop_offsets: BTreeMap<usize, usize>, /* offset -> diff_id */
    pub call_offsets: BTreeMap<usize, usize>, /* offset -> diff_id */
}

#[derive(Clone, Debug)]
pub struct ModuleStateMap {
    pub local_functions: BTreeMap<usize, FunctionStateMap>,
    pub total_size: usize,
}

#[derive(Clone, Debug)]
pub struct DenseArrayMap<T: Clone + Debug> {
    pub elements: Vec<Option<T>>,
}

impl<T: Clone + Debug> DenseArrayMap<T> {
    pub fn new() -> DenseArrayMap<T> {
        DenseArrayMap { elements: vec![] }
    }

    pub fn set(&mut self, idx: usize, elem: T) {
        while self.elements.len() < idx + 1 {
            self.elements.push(None);
        }
        self.elements[idx] = Some(elem);
    }

    pub fn into_vec(self) -> Option<Vec<T>> {
        let mut ret: Vec<T> = Vec::with_capacity(self.elements.len());
        for elem in self.elements {
            if elem.is_none() {
                return None;
            }
            ret.push(elem.unwrap());
        }
        Some(ret)
    }
}

#[derive(Clone, Debug)]
pub struct WasmFunctionState {
    stack: Vec<Option<u64>>,
    locals: Vec<u64>,
}

impl ModuleStateMap {
    pub fn lookup_call_ip(
        &self,
        ip: usize,
        base: usize,
    ) -> Option<(&FunctionStateMap, MachineState)> {
        if ip < base || ip - base >= self.total_size {
            None
        } else {
            //println!("lookup ip: {} in {:?}", ip - base, self.local_functions);
            let fsm = self
                .local_functions
                .range((Unbounded, Included(&(ip - base))))
                .last()
                .map(|x| x.1)
                .unwrap();
            Some((
                fsm,
                fsm.call_offsets
                    .get(&(ip - base))
                    .map(|x| fsm.diffs[*x].build_state(fsm))
                    .unwrap(),
            ))
        }
    }
}

impl FunctionStateMap {
    pub fn new(initial: MachineState, shadow_size: usize) -> FunctionStateMap {
        FunctionStateMap {
            initial,
            shadow_size,
            diffs: vec![],
            loop_offsets: BTreeMap::new(),
            call_offsets: BTreeMap::new(),
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
        MachineStateDiff {
            last: None,
            stack_push: self.stack_values[first_diff_stack_depth..].to_vec(),
            stack_pop: old.stack_values.len() - first_diff_stack_depth,
            reg_diff: reg_diff,
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
        }
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
        }
    }

    #[warn(unused_variables)]
    pub unsafe fn read_stack(msm: &ModuleStateMap, code_base: usize, mut stack: *const u64) {
        let r15 = *stack;
        let r14 = *stack.offset(1);
        let r13 = *stack.offset(2);
        let r12 = *stack.offset(3);
        let rbx = *stack.offset(4);
        stack = stack.offset(5);

        let mut next_known_registers: [Option<u64>; 24] = [None; 24];
        next_known_registers[X64Register::GPR(GPR::R15).to_index().0] = Some(r15);
        next_known_registers[X64Register::GPR(GPR::R14).to_index().0] = Some(r14);
        next_known_registers[X64Register::GPR(GPR::R13).to_index().0] = Some(r13);
        next_known_registers[X64Register::GPR(GPR::R12).to_index().0] = Some(r12);
        next_known_registers[X64Register::GPR(GPR::RBX).to_index().0] = Some(rbx);

        for i in 0.. {
            let known_registers = ::std::mem::replace(&mut next_known_registers, [None; 24]);
            let mut wasm_stack: DenseArrayMap<u64> = DenseArrayMap::new();
            let mut wasm_locals: DenseArrayMap<u64> = DenseArrayMap::new();
            let ret_addr = *stack;
            stack = stack.offset(1);
            let (fsm, state) = match msm.lookup_call_ip(ret_addr as usize, code_base) {
                Some(x) => x,
                _ => break,
            };
            let mut found_shadow = false;
            for v in state.stack_values.iter().rev() {
                match *v {
                    MachineValue::ExplicitShadow => {
                        stack = stack.offset((fsm.shadow_size / 8) as isize);
                        found_shadow = true;
                    }
                    MachineValue::Undefined => {
                        stack = stack.offset(1);
                    }
                    MachineValue::PreserveRegister(idx) => {
                        next_known_registers[idx.0] = Some(*stack);
                        stack = stack.offset(1);
                    }
                    MachineValue::CopyStackBPRelative(offset) => {
                        stack = stack.offset(1);
                    }
                    MachineValue::WasmStack(idx) => {
                        wasm_stack.set(idx, *stack);
                        stack = stack.offset(1);
                    }
                    MachineValue::WasmLocal(idx) => {
                        wasm_locals.set(idx, *stack);
                        stack = stack.offset(1);
                    }
                }
            }
            for (i, v) in state.register_values.iter().enumerate() {
                match *v {
                    MachineValue::Undefined => {}
                    MachineValue::WasmStack(idx) => {
                        if let Some(v) = known_registers[i] {
                            wasm_stack.set(idx, v);
                        }
                    }
                    MachineValue::WasmLocal(idx) => {
                        if let Some(v) = known_registers[i] {
                            wasm_locals.set(idx, v);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            assert_eq!(found_shadow, true);
            stack = stack.offset(1); // RBP

            let wfs = WasmFunctionState {
                stack: wasm_stack.elements,
                locals: wasm_locals.into_vec().unwrap(),
            };
            println!("Frame #{}: {:p} {:?}", i, ret_addr as *const u8, wfs);
        }
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
