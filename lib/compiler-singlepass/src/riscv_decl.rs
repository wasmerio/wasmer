//! RISC-V structures.

// TODO: handle warnings
#![allow(unused_variables, unused_imports)]

use crate::{
    common_decl::{MachineState, MachineValue, RegisterIndex},
    location::{CombinedRegister, Reg as AbstractReg},
};
use std::{collections::BTreeMap, slice::Iter};
use wasmer_types::target::CallingConvention;
use wasmer_types::{CompileError, Type};

/*
+-----+-----------+-------------------------------+-------------------+
| Reg | ABI Name  | Description                   | Saved by Callee   |
+-----+-----------+-------------------------------+-------------------+
| x0  | zero      | hardwired zero                | -                 |
| x1  | ra        | return address                | -R                |
| x2  | sp        | stack pointer                 | -E                |
| x3  | gp        | global pointer                | -                 |
| x4  | tp        | thread pointer                | -                 |
| x5  | t0        | temporary register 0          | -R                |
| x6  | t1        | temporary register 1          | -R                |
| x7  | t2        | temporary register 2          | -R                |
| x8  | s0/fp     | saved register 0/frame pointer| -E                |
| x9  | s1        | saved register 1              | -E                |
| x10 | a0        | function arg 0/return value 0 | -R                |
| x11 | a1        | function arg 1/return value 1 | -R                |
| x12 | a2        | function argument 2           | -R                |
| x13 | a3        | function argument 3           | -R                |
| x14 | a4        | function argument 4           | -R                |
| x15 | a5        | function argument 5           | -R                |
| x16 | a6        | function argument 6           | -R                |
| x17 | a7        | function argument 7           | -R                |
| x18 | s2        | saved register 2              | -E                |
| x19 | s3        | saved register 3              | -E                |
| x20 | s4        | saved register 4              | -E                |
| x21 | s5        | saved register 5              | -E                |
| x22 | s6        | saved register 6              | -E                |
| x23 | s7        | saved register 7              | -E                |
| x24 | s8        | saved register 8              | -E                |
| x25 | s9        | saved register 9              | -E                |
| x26 | s10       | saved register 10             | -E                |
| x27 | s11       | saved register 11             | -E                |
| x28 | t3        | temporary register 3          | -R                |
| x29 | t4        | temporary register 4          | -R                |
| x30 | t5        | temporary register 5          | -R                |
| x31 | t6        | temporary register 6          | -R                |
+-----+-----------+-------------------------------+-------------------+
Legend: -R = caller-saved, -E = callee-saved, - = not saved
*/

/// General-purpose registers.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub enum GPR {
    X0 = 0,
    X1 = 1,
    Sp = 2,
    X3 = 3,
    X4 = 4,
    X5 = 5,
    X6 = 6,
    X7 = 7,
    Fp = 8,
    X9 = 9,
    X10 = 10,
    X11 = 11,
    X12 = 12,
    X13 = 13,
    X14 = 14,
    X15 = 15,
    X16 = 16,
    X17 = 17,
    X18 = 18,
    X19 = 19,
    X20 = 20,
    X21 = 21,
    X22 = 22,
    X23 = 23,
    X24 = 24,
    X25 = 25,
    X26 = 26,
    X27 = 27,
    X28 = 28,
    X29 = 29,
    X30 = 30,
    X31 = 31,
}

impl AbstractReg for GPR {
    fn is_callee_save(self) -> bool {
        todo!();
    }
    fn is_reserved(self) -> bool {
        todo!();
    }
    fn into_index(self) -> usize {
        self as usize
    }
    fn from_index(n: usize) -> Result<GPR, ()> {
        match n {
            0..=31 => Ok(*GPR::iterator().nth(n).unwrap()),
            _ => Err(()),
        }
    }
    fn iterator() -> Iter<'static, GPR> {
        static GPRS: [GPR; 32] = [
            GPR::X0,
            GPR::X1,
            GPR::Sp,
            GPR::X3,
            GPR::X4,
            GPR::X5,
            GPR::X6,
            GPR::X7,
            GPR::Fp,
            GPR::X9,
            GPR::X10,
            GPR::X11,
            GPR::X12,
            GPR::X13,
            GPR::X14,
            GPR::X15,
            GPR::X16,
            GPR::X17,
            GPR::X18,
            GPR::X19,
            GPR::X20,
            GPR::X21,
            GPR::X22,
            GPR::X23,
            GPR::X24,
            GPR::X25,
            GPR::X26,
            GPR::X27,
            GPR::X28,
            GPR::X29,
            GPR::X30,
            GPR::X31,
        ];
        GPRS.iter()
    }
    fn to_dwarf(self) -> u16 {
        todo!();
    }
}

/*
+-----+-------+--------------------------+-------------------+
| Reg | Name  | Description              | Saved by          |
+-----+-------+--------------------------+-------------------+
| f0  | ft0   | FP temporary             | Caller            |
| f1  | ft1   | FP temporary             | Caller            |
| f2  | ft2   | FP temporary             | Caller            |
| f3  | ft3   | FP temporary             | Caller            |
| f4  | ft4   | FP temporary             | Caller            |
| f5  | ft5   | FP temporary             | Caller            |
| f6  | ft6   | FP temporary             | Caller            |
| f7  | ft7   | FP temporary             | Caller            |
| f8  | fs0   | FP saved register        | Callee            |
| f9  | fs1   | FP saved register        | Callee            |
| f10 | fa0   | FP argument/return value | Caller            |
| f11 | fa1   | FP argument/return value | Caller            |
| f12 | fa2   | FP argument              | Caller            |
| f13 | fa3   | FP argument              | Caller            |
| f14 | fa4   | FP argument              | Caller            |
| f15 | fa5   | FP argument              | Caller            |
| f16 | fa6   | FP argument              | Caller            |
| f17 | fa7   | FP argument              | Caller            |
| f18 | fs2   | FP saved register        | Callee            |
| f19 | fs3   | FP saved register        | Callee            |
| f20 | fs4   | FP saved register        | Callee            |
| f21 | fs5   | FP saved register        | Callee            |
| f22 | fs6   | FP saved register        | Callee            |
| f23 | fs7   | FP saved register        | Callee            |
| f24 | fs8   | FP saved register        | Callee            |
| f25 | fs9   | FP saved register        | Callee            |
| f26 | fs10  | FP saved register        | Callee            |
| f27 | fs11  | FP saved register        | Callee            |
| f28 | ft8   | FP temporary             | Caller            |
| f29 | ft9   | FP temporary             | Caller            |
| f30 | ft10  | FP temporary             | Caller            |
| f31 | ft11  | FP temporary             | Caller            |
+-----+-------+--------------------------+-------------------+
Legend: FP = floating-point
*/

/// Floating-point registers.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub enum FPR {
    F0 = 0,
    F1 = 1,
    F2 = 2,
    F3 = 3,
    F4 = 4,
    F5 = 5,
    F6 = 6,
    F7 = 7,
    F8 = 8,
    F9 = 9,
    F10 = 10,
    F11 = 11,
    F12 = 12,
    F13 = 13,
    F14 = 14,
    F15 = 15,
    F16 = 16,
    F17 = 17,
    F18 = 18,
    F19 = 19,
    F20 = 20,
    F21 = 21,
    F22 = 22,
    F23 = 23,
    F24 = 24,
    F25 = 25,
    F26 = 26,
    F27 = 27,
    F28 = 28,
    F29 = 29,
    F30 = 30,
    F31 = 31,
}

impl AbstractReg for FPR {
    fn is_callee_save(self) -> bool {
        // TODO: implement callee-save registers for FPR.
        todo!()
    }
    fn is_reserved(self) -> bool {
        // TODO: implement reserved floating-point registers.
        todo!()
    }
    fn into_index(self) -> usize {
        self as usize
    }
    fn from_index(n: usize) -> Result<FPR, ()> {
        match n {
            0..=31 => Ok(*FPR::iterator().nth(n).unwrap()),
            _ => Err(()),
        }
    }
    fn iterator() -> Iter<'static, FPR> {
        static FPRS: [FPR; 32] = [
            FPR::F0,
            FPR::F1,
            FPR::F2,
            FPR::F3,
            FPR::F4,
            FPR::F5,
            FPR::F6,
            FPR::F7,
            FPR::F8,
            FPR::F9,
            FPR::F10,
            FPR::F11,
            FPR::F12,
            FPR::F13,
            FPR::F14,
            FPR::F15,
            FPR::F16,
            FPR::F17,
            FPR::F18,
            FPR::F19,
            FPR::F20,
            FPR::F21,
            FPR::F22,
            FPR::F23,
            FPR::F24,
            FPR::F25,
            FPR::F26,
            FPR::F27,
            FPR::F28,
            FPR::F29,
            FPR::F30,
            FPR::F31,
        ];
        FPRS.iter()
    }
    fn to_dwarf(self) -> u16 {
        // TODO: map FPR register to DWARF register number.
        todo!()
    }
}

/// A combined RISC-V register.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum RiscvRegister {
    /// General-purpose register.
    GPR(GPR),
    /// Floating-point register.
    FPR(FPR),
}

impl CombinedRegister for RiscvRegister {
    fn to_index(&self) -> RegisterIndex {
        match *self {
            RiscvRegister::GPR(x) => RegisterIndex(x as usize),
            RiscvRegister::FPR(x) => RegisterIndex(x as usize + 64),
        }
    }
    fn from_gpr(x: u16) -> Self {
        RiscvRegister::GPR(GPR::from_index(x as usize).unwrap())
    }
    fn from_simd(x: u16) -> Self {
        RiscvRegister::FPR(FPR::from_index(x as usize).unwrap())
    }
    fn _from_dwarf_regnum(x: u16) -> Option<Self> {
        // TODO: map DWARF register number to RiscvRegister
        None
    }
}

/// Allocator for function argument registers according to the RISC-V ABI.
#[derive(Default)]
pub struct ArgumentRegisterAllocator {
    // TODO: track next GPR/FPR for argument passing.
}

impl ArgumentRegisterAllocator {
    /// Allocates a register for argument type `ty`. Returns `None` if no register is available.
    #[allow(dead_code)]
    pub fn next(
        &mut self,
        ty: Type,
        calling_convention: CallingConvention,
    ) -> Result<Option<RiscvRegister>, CompileError> {
        // TODO: implement RISC-V calling convention register allocation.
        todo!()
    }
}

/// Create a new `MachineState` with default values for RISC-V.
pub fn new_machine_state() -> MachineState {
    MachineState {
        stack_values: vec![],
        register_values: vec![MachineValue::Undefined; 32 + 32],
        prev_frame: BTreeMap::new(),
        wasm_stack: vec![],
        wasm_inst_offset: usize::MAX,
    }
}
