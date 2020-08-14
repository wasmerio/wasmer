//! X64 structures.

use crate::common_decl::{MachineState, MachineValue, RegisterIndex};
use std::collections::BTreeMap;
use wasmer_types::Type;

/// General-purpose registers.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum GPR {
    /// RAX register
    RAX,
    /// RCX register
    RCX,
    /// RDX register
    RDX,
    /// RBX register
    RBX,
    /// RSP register
    RSP,
    /// RBP register
    RBP,
    /// RSI register
    RSI,
    /// RDI register
    RDI,
    /// R8 register
    R8,
    /// R9 register
    R9,
    /// R10 register
    R10,
    /// R11 register
    R11,
    /// R12 register
    R12,
    /// R13 register
    R13,
    /// R14 register
    R14,
    /// R15 register
    R15,
}

/// XMM registers.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(dead_code)]
pub enum XMM {
    /// XMM register 0
    XMM0,
    /// XMM register 1
    XMM1,
    /// XMM register 2
    XMM2,
    /// XMM register 3
    XMM3,
    /// XMM register 4
    XMM4,
    /// XMM register 5
    XMM5,
    /// XMM register 6
    XMM6,
    /// XMM register 7
    XMM7,
    /// XMM register 8
    XMM8,
    /// XMM register 9
    XMM9,
    /// XMM register 10
    XMM10,
    /// XMM register 11
    XMM11,
    /// XMM register 12
    XMM12,
    /// XMM register 13
    XMM13,
    /// XMM register 14
    XMM14,
    /// XMM register 15
    XMM15,
}

/// A machine register under the x86-64 architecture.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum X64Register {
    /// General-purpose registers.
    GPR(GPR),
    /// XMM (floating point/SIMD) registers.
    XMM(XMM),
}

impl X64Register {
    /// Returns the index of the register.
    pub fn to_index(&self) -> RegisterIndex {
        match *self {
            X64Register::GPR(x) => RegisterIndex(x as usize),
            X64Register::XMM(x) => RegisterIndex(x as usize + 16),
        }
    }

    /// Converts a DWARF regnum to X64Register.
    pub fn _from_dwarf_regnum(x: u16) -> Option<X64Register> {
        Some(match x {
            0 => X64Register::GPR(GPR::RAX),
            1 => X64Register::GPR(GPR::RDX),
            2 => X64Register::GPR(GPR::RCX),
            3 => X64Register::GPR(GPR::RBX),
            4 => X64Register::GPR(GPR::RSI),
            5 => X64Register::GPR(GPR::RDI),
            6 => X64Register::GPR(GPR::RBP),
            7 => X64Register::GPR(GPR::RSP),
            8 => X64Register::GPR(GPR::R8),
            9 => X64Register::GPR(GPR::R9),
            10 => X64Register::GPR(GPR::R10),
            11 => X64Register::GPR(GPR::R11),
            12 => X64Register::GPR(GPR::R12),
            13 => X64Register::GPR(GPR::R13),
            14 => X64Register::GPR(GPR::R14),
            15 => X64Register::GPR(GPR::R15),

            17 => X64Register::XMM(XMM::XMM0),
            18 => X64Register::XMM(XMM::XMM1),
            19 => X64Register::XMM(XMM::XMM2),
            20 => X64Register::XMM(XMM::XMM3),
            21 => X64Register::XMM(XMM::XMM4),
            22 => X64Register::XMM(XMM::XMM5),
            23 => X64Register::XMM(XMM::XMM6),
            24 => X64Register::XMM(XMM::XMM7),
            _ => return None,
        })
    }

    /// Returns the instruction prefix for `movq %this_reg, ?(%rsp)`.
    ///
    /// To build an instruction, append the memory location as a 32-bit
    /// offset to the stack pointer to this prefix.
    pub fn _prefix_mov_to_stack(&self) -> Option<&'static [u8]> {
        Some(match *self {
            X64Register::GPR(gpr) => match gpr {
                GPR::RDI => &[0x48, 0x89, 0xbc, 0x24],
                GPR::RSI => &[0x48, 0x89, 0xb4, 0x24],
                GPR::RDX => &[0x48, 0x89, 0x94, 0x24],
                GPR::RCX => &[0x48, 0x89, 0x8c, 0x24],
                GPR::R8 => &[0x4c, 0x89, 0x84, 0x24],
                GPR::R9 => &[0x4c, 0x89, 0x8c, 0x24],
                _ => return None,
            },
            X64Register::XMM(xmm) => match xmm {
                XMM::XMM0 => &[0x66, 0x0f, 0xd6, 0x84, 0x24],
                XMM::XMM1 => &[0x66, 0x0f, 0xd6, 0x8c, 0x24],
                XMM::XMM2 => &[0x66, 0x0f, 0xd6, 0x94, 0x24],
                XMM::XMM3 => &[0x66, 0x0f, 0xd6, 0x9c, 0x24],
                XMM::XMM4 => &[0x66, 0x0f, 0xd6, 0xa4, 0x24],
                XMM::XMM5 => &[0x66, 0x0f, 0xd6, 0xac, 0x24],
                XMM::XMM6 => &[0x66, 0x0f, 0xd6, 0xb4, 0x24],
                XMM::XMM7 => &[0x66, 0x0f, 0xd6, 0xbc, 0x24],
                _ => return None,
            },
        })
    }
}

/// An allocator that allocates registers for function arguments according to the System V ABI.
#[derive(Default)]
pub struct ArgumentRegisterAllocator {
    n_gprs: usize,
    n_xmms: usize,
}

impl ArgumentRegisterAllocator {
    /// Allocates a register for argument type `ty`. Returns `None` if no register is available for this type.
    pub fn next(&mut self, ty: Type) -> Option<X64Register> {
        static GPR_SEQ: &'static [GPR] =
            &[GPR::RDI, GPR::RSI, GPR::RDX, GPR::RCX, GPR::R8, GPR::R9];
        static XMM_SEQ: &'static [XMM] = &[
            XMM::XMM0,
            XMM::XMM1,
            XMM::XMM2,
            XMM::XMM3,
            XMM::XMM4,
            XMM::XMM5,
            XMM::XMM6,
            XMM::XMM7,
        ];
        match ty {
            Type::I32 | Type::I64 => {
                if self.n_gprs < GPR_SEQ.len() {
                    let gpr = GPR_SEQ[self.n_gprs];
                    self.n_gprs += 1;
                    Some(X64Register::GPR(gpr))
                } else {
                    None
                }
            }
            Type::F32 | Type::F64 => {
                if self.n_xmms < XMM_SEQ.len() {
                    let xmm = XMM_SEQ[self.n_xmms];
                    self.n_xmms += 1;
                    Some(X64Register::XMM(xmm))
                } else {
                    None
                }
            }
            _ => todo!(
                "ArgumentRegisterAllocator::next: Unsupported type: {:?}",
                ty
            ),
        }
    }
}

/// Create a new `MachineState` with default values.
pub fn new_machine_state() -> MachineState {
    MachineState {
        stack_values: vec![],
        register_values: vec![MachineValue::Undefined; 16 + 8],
        prev_frame: BTreeMap::new(),
        wasm_stack: vec![],
        wasm_inst_offset: std::usize::MAX,
    }
}
