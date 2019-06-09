#[derive(Copy, Clone, Debug)]
pub struct RegisterIndex(pub usize);

#[derive(Clone, Debug, Default)]
pub struct FunctionStateMap {
    pub local_to_locations: Vec<Location>,
    pub diffs: Vec<StateDiff>,
}

#[derive(Clone, Debug, Default)]
pub struct StateDiff {
    pub last: Option<usize>,
    pub stack_to_locations_push: Vec<Location>,
    pub stack_to_locations_pop: usize,
}

#[derive(Clone, Debug)]
pub enum Location {
    Virtual, // no physical storage
    Memory(RegisterIndex, i32),
    Register(RegisterIndex),
}

#[cfg(all(unix, target_arch = "x86_64"))]
pub mod x64 {
    use super::*;

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
                X64Register::XMM(x) => RegisterIndex(x as usize + 1000),
            }
        }
    }
}
