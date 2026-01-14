//! RISC-V structures.

use crate::location::{CombinedRegister, Reg as AbstractReg};
use std::slice::Iter;
use wasmer_types::{CompileError, Type};

/*

Register definition: https://en.wikichip.org/wiki/risc-v/registers#Calling_convention

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
    XZero = 0,
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

impl From<GPR> for u8 {
    fn from(val: GPR) -> Self {
        val as u8
    }
}

impl AbstractReg for GPR {
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
            GPR::XZero,
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
    #[cfg(feature = "unwind")]
    fn to_dwarf(self) -> gimli::Register {
        use gimli::RiscV;

        match self {
            GPR::XZero => RiscV::ZERO,
            GPR::X1 => RiscV::X1,
            GPR::Sp => RiscV::SP,
            GPR::X3 => RiscV::X3,
            GPR::X4 => RiscV::X4,
            GPR::X5 => RiscV::X5,
            GPR::X6 => RiscV::X6,
            GPR::X7 => RiscV::X7,
            // TODO: use new constant: https://github.com/gimli-rs/gimli/pull/802
            GPR::Fp => RiscV::X8,
            GPR::X9 => RiscV::X9,
            GPR::X10 => RiscV::X10,
            GPR::X11 => RiscV::X11,
            GPR::X12 => RiscV::X12,
            GPR::X13 => RiscV::X13,
            GPR::X14 => RiscV::X14,
            GPR::X15 => RiscV::X15,
            GPR::X16 => RiscV::X16,
            GPR::X17 => RiscV::X17,
            GPR::X18 => RiscV::X18,
            GPR::X19 => RiscV::X19,
            GPR::X20 => RiscV::X20,
            GPR::X21 => RiscV::X21,
            GPR::X22 => RiscV::X22,
            GPR::X23 => RiscV::X23,
            GPR::X24 => RiscV::X24,
            GPR::X25 => RiscV::X25,
            GPR::X26 => RiscV::X26,
            GPR::X27 => RiscV::X27,
            GPR::X28 => RiscV::X28,
            GPR::X29 => RiscV::X29,
            GPR::X30 => RiscV::X30,
            GPR::X31 => RiscV::X31,
        }
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

impl From<FPR> for u8 {
    fn from(val: FPR) -> Self {
        val as u8
    }
}

impl AbstractReg for FPR {
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

    #[cfg(feature = "unwind")]
    fn to_dwarf(self) -> gimli::Register {
        use gimli::RiscV;

        match self {
            FPR::F0 => RiscV::F0,
            FPR::F1 => RiscV::F1,
            FPR::F2 => RiscV::F2,
            FPR::F3 => RiscV::F3,
            FPR::F4 => RiscV::F4,
            FPR::F5 => RiscV::F5,
            FPR::F6 => RiscV::F6,
            FPR::F7 => RiscV::F7,
            FPR::F8 => RiscV::F8,
            FPR::F9 => RiscV::F9,
            FPR::F10 => RiscV::F10,
            FPR::F11 => RiscV::F11,
            FPR::F12 => RiscV::F12,
            FPR::F13 => RiscV::F13,
            FPR::F14 => RiscV::F14,
            FPR::F15 => RiscV::F15,
            FPR::F16 => RiscV::F16,
            FPR::F17 => RiscV::F17,
            FPR::F18 => RiscV::F18,
            FPR::F19 => RiscV::F19,
            FPR::F20 => RiscV::F20,
            FPR::F21 => RiscV::F21,
            FPR::F22 => RiscV::F22,
            FPR::F23 => RiscV::F23,
            FPR::F24 => RiscV::F24,
            FPR::F25 => RiscV::F25,
            FPR::F26 => RiscV::F26,
            FPR::F27 => RiscV::F27,
            FPR::F28 => RiscV::F28,
            FPR::F29 => RiscV::F29,
            FPR::F30 => RiscV::F30,
            FPR::F31 => RiscV::F31,
        }
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
    fn from_gpr(x: u16) -> Self {
        RiscvRegister::GPR(GPR::from_index(x as usize).unwrap())
    }
    fn from_simd(x: u16) -> Self {
        RiscvRegister::FPR(FPR::from_index(x as usize).unwrap())
    }
}

/// Allocator for function argument registers according to the RISC-V ABI.
#[derive(Default)]
pub struct ArgumentRegisterAllocator {
    n_gprs: usize,
    n_fprs: usize,
}

impl ArgumentRegisterAllocator {
    /// Allocates a register for argument type `ty`. Returns `None` if no register is available.
    #[allow(dead_code)]
    pub fn next(&mut self, ty: Type) -> Result<Option<RiscvRegister>, CompileError> {
        let ret = {
            static GPR_SEQ: &[GPR] = &[
                GPR::X10,
                GPR::X11,
                GPR::X12,
                GPR::X13,
                GPR::X14,
                GPR::X15,
                GPR::X16,
                GPR::X17,
            ];
            static FPR_SEQ: &[FPR] = &[
                FPR::F10,
                FPR::F11,
                FPR::F12,
                FPR::F13,
                FPR::F14,
                FPR::F15,
                FPR::F16,
                FPR::F17,
            ];
            match ty {
                Type::I32 | Type::I64 => {
                    if self.n_gprs < GPR_SEQ.len() {
                        let gpr = GPR_SEQ[self.n_gprs];
                        self.n_gprs += 1;
                        Some(RiscvRegister::GPR(gpr))
                    } else {
                        None
                    }
                }
                Type::F32 | Type::F64 => {
                    if self.n_fprs < FPR_SEQ.len() {
                        let neon = FPR_SEQ[self.n_fprs];
                        self.n_fprs += 1;
                        Some(RiscvRegister::FPR(neon))
                    } else {
                        None
                    }
                }
                _ => {
                    return Err(CompileError::Codegen(format!(
                        "No register available for type {ty}"
                    )));
                }
            }
        };

        Ok(ret)
    }
}
