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

/// Floating-point registers.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub enum FPR {
    // TODO: define floating-point registers F0-F31.
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
        // TODO: map index to FPR.
        todo!()
    }
    fn iterator() -> Iter<'static, FPR> {
        // TODO: return an iterator over all FPR variants.
        todo!()
    }
    fn to_dwarf(self) -> u16 {
        // TODO: map FPR register to DWARF register number.
        todo!()
    }
}

/// A combined RISC-V register.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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
            RiscvRegister::FPR(x) => RegisterIndex(x as usize + /* FPR offset */ 0),
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
        register_values: vec![MachineValue::Undefined; 32 /* TODO: add FPR count */],
        prev_frame: BTreeMap::new(),
        wasm_stack: vec![],
        wasm_inst_offset: usize::MAX,
    }
}
