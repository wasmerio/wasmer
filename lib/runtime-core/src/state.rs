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
    WasmStack(usize),
    WasmLocal(usize),
}

#[derive(Clone, Debug)]
pub struct FunctionStateMap {
    pub initial: MachineState,
    pub diffs: Vec<MachineStateDiff>,
}

impl FunctionStateMap {
    pub fn new(initial: MachineState) -> FunctionStateMap {
        FunctionStateMap {
            initial,
            diffs: vec![],
        }
    }
}

impl MachineState {
    pub fn diff(&self, old: &MachineState) -> MachineStateDiff {
        let first_diff_stack_depth: usize = self.stack_values.iter().zip(old.stack_values.iter()).enumerate()
            .find(|&(_, (&a, &b))| a != b).map(|x| x.0)
            .unwrap_or(old.stack_values.len().min(self.stack_values.len()));
        assert_eq!(self.register_values.len(), old.register_values.len());
        let reg_diff: Vec<_> = self.register_values.iter().zip(old.register_values.iter()).enumerate()
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
