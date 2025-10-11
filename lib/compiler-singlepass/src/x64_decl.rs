//! X64 structures.

#![allow(clippy::upper_case_acronyms)]
use crate::common_decl::{MachineState, MachineValue, RegisterIndex};
use crate::location::CombinedRegister;
use crate::location::Reg as AbstractReg;
use std::collections::BTreeMap;
use std::slice::Iter;
use wasmer_types::{CompileError, Type, target::CallingConvention};

/// General-purpose registers.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum GPR {
    RAX = 0,
    RCX = 1,
    RDX = 2,
    RBX = 3,
    RSP = 4,
    RBP = 5,
    RSI = 6,
    RDI = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

impl From<GPR> for u8 {
    fn from(val: GPR) -> Self {
        val as u8
    }
}

/// XMM registers.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(dead_code)]
pub enum XMM {
    XMM0 = 0,
    XMM1 = 1,
    XMM2 = 2,
    XMM3 = 3,
    XMM4 = 4,
    XMM5 = 5,
    XMM6 = 6,
    XMM7 = 7,
    XMM8 = 8,
    XMM9 = 9,
    XMM10 = 10,
    XMM11 = 11,
    XMM12 = 12,
    XMM13 = 13,
    XMM14 = 14,
    XMM15 = 15,
}

impl From<XMM> for u8 {
    fn from(val: XMM) -> Self {
        val as u8
    }
}

impl AbstractReg for GPR {
    fn is_callee_save(self) -> bool {
        const IS_CALLEE_SAVE: [bool; 16] = [
            false, false, false, true, true, true, false, false, false, false, false, false, true,
            true, true, true,
        ];
        IS_CALLEE_SAVE[self as usize]
    }
    fn is_reserved(self) -> bool {
        self == GPR::RSP || self == GPR::RBP || self == GPR::R10 || self == GPR::R15
    }
    fn into_index(self) -> usize {
        self as usize
    }
    fn from_index(n: usize) -> Result<GPR, ()> {
        match n {
            0..=15 => Ok(*GPR::iterator().nth(n).unwrap()),
            _ => Err(()),
        }
    }
    fn iterator() -> Iter<'static, GPR> {
        static GPRS: [GPR; 16] = [
            GPR::RAX,
            GPR::RCX,
            GPR::RDX,
            GPR::RBX,
            GPR::RSP,
            GPR::RBP,
            GPR::RSI,
            GPR::RDI,
            GPR::R8,
            GPR::R9,
            GPR::R10,
            GPR::R11,
            GPR::R12,
            GPR::R13,
            GPR::R14,
            GPR::R15,
        ];
        GPRS.iter()
    }
    #[cfg(feature = "unwind")]
    fn to_dwarf(self) -> gimli::Register {
        use gimli::X86_64;

        match self {
            GPR::RAX => X86_64::RAX,
            GPR::RCX => X86_64::RCX,
            GPR::RDX => X86_64::RDX,
            GPR::RBX => X86_64::RBX,
            GPR::RSP => X86_64::RSP,
            GPR::RBP => X86_64::RBP,
            GPR::RSI => X86_64::RSI,
            GPR::RDI => X86_64::RDI,
            GPR::R8 => X86_64::R8,
            GPR::R9 => X86_64::R9,
            GPR::R10 => X86_64::R10,
            GPR::R11 => X86_64::R11,
            GPR::R12 => X86_64::R12,
            GPR::R13 => X86_64::R13,
            GPR::R14 => X86_64::R14,
            GPR::R15 => X86_64::R15,
        }
    }
}

impl AbstractReg for XMM {
    fn is_callee_save(self) -> bool {
        const IS_CALLEE_SAVE: [bool; 16] = [
            false, false, false, false, false, false, false, false, true, true, true, true, true,
            true, true, true,
        ];
        IS_CALLEE_SAVE[self as usize]
    }
    fn is_reserved(self) -> bool {
        false
    }
    fn into_index(self) -> usize {
        self as usize
    }
    fn from_index(n: usize) -> Result<XMM, ()> {
        match n {
            0..=15 => Ok(*XMM::iterator().nth(n).unwrap()),
            _ => Err(()),
        }
    }
    fn iterator() -> Iter<'static, XMM> {
        static XMMS: [XMM; 16] = [
            XMM::XMM0,
            XMM::XMM1,
            XMM::XMM2,
            XMM::XMM3,
            XMM::XMM4,
            XMM::XMM5,
            XMM::XMM6,
            XMM::XMM7,
            XMM::XMM8,
            XMM::XMM9,
            XMM::XMM10,
            XMM::XMM11,
            XMM::XMM12,
            XMM::XMM13,
            XMM::XMM14,
            XMM::XMM15,
        ];
        XMMS.iter()
    }
    #[cfg(feature = "unwind")]
    fn to_dwarf(self) -> gimli::Register {
        use gimli::X86_64;

        match self {
            XMM::XMM0 => X86_64::XMM0,
            XMM::XMM1 => X86_64::XMM1,
            XMM::XMM2 => X86_64::XMM2,
            XMM::XMM3 => X86_64::XMM3,
            XMM::XMM4 => X86_64::XMM4,
            XMM::XMM5 => X86_64::XMM5,
            XMM::XMM6 => X86_64::XMM6,
            XMM::XMM7 => X86_64::XMM7,
            XMM::XMM8 => X86_64::XMM8,
            XMM::XMM9 => X86_64::XMM9,
            XMM::XMM10 => X86_64::XMM10,
            XMM::XMM11 => X86_64::XMM11,
            XMM::XMM12 => X86_64::XMM12,
            XMM::XMM13 => X86_64::XMM13,
            XMM::XMM14 => X86_64::XMM14,
            XMM::XMM15 => X86_64::XMM15,
        }
    }
}

/// A machine register under the x86-64 architecture.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum X64Register {
    /// General-purpose registers.
    GPR(GPR),
    /// XMM (floating point/SIMD) registers.
    XMM(XMM),
}

impl CombinedRegister for X64Register {
    /// Returns the index of the register.
    fn to_index(&self) -> RegisterIndex {
        match *self {
            X64Register::GPR(x) => RegisterIndex(x as usize),
            X64Register::XMM(x) => RegisterIndex(x as usize + 16),
        }
    }
    /// Convert from a GPR register
    fn from_gpr(x: u16) -> Self {
        X64Register::GPR(GPR::from_index(x as usize).unwrap())
    }
    /// Convert from an SIMD register
    fn from_simd(x: u16) -> Self {
        X64Register::XMM(XMM::from_index(x as usize).unwrap())
    }

    /* x86_64-abi-0.99.pdf
     * Register Name                    | Number | Abbreviation
     * General Purpose Register RAX     | 0      | %rax
     * General Purpose Register RDX     | 1      | %rdx
     * General Purpose Register RCX     | 2      | %rcx
     * General Purpose Register RBX     | 3      | %rbx
     * General Purpose Register RSI     | 4      | %rsi
     * General Purpose Register RDI     | 5      | %rdi
     * Frame Pointer Register   RBP     | 6      | %rbp
     * Stack Pointer Register   RSP     | 7      | %rsp
     * Extended Integer Registers 8-15  | 8-15   | %r8-%r15
     * Return Address RA                | 16     |
     * Vector Registers 0-7             | 17-24  | %xmm0-%xmm7
     * Extended Vector Registers 8-15   | 25-32  | %xmm8-%xmm15
     * Floating Point Registers 0-7     | 33-40  | %st0-%st7
     * MMX Registers 0-7                | 41-48  | %mm0-%mm7
     * Flag Register                    | 49     | %rFLAGS
     * Segment Register ES              | 50     | %es
     * Segment Register CS              | 51     | %cs
     * Segment Register SS              | 52     | %ss
     * Segment Register DS              | 53     | %ds
     * Segment Register FS              | 54     | %fs
     * Segment Register GS              | 55     | %gs
     * Reserved                         | 56-57  |
     * FS Base address                  | 58     | %fs.base
     * GS Base address                  | 59     | %gs.base
     * Reserved                         | 60-61  |
     * Task Register                    | 62     | %tr
     * LDT Register                     | 63     | %ldtr
     * 128-bit Media Control and Status | 64     | %mxcsr
     * x87 Control Word                 | 65     | %fcw
     * x87 Status Word                  | 66     | %fsw
     */
}

/// An allocator that allocates registers for function arguments according to the System V ABI.
#[derive(Default)]
pub struct ArgumentRegisterAllocator {
    n_gprs: usize,
    n_xmms: usize,
}

impl ArgumentRegisterAllocator {
    /// Allocates a register for argument type `ty`. Returns `None` if no register is available for this type.
    pub fn next(
        &mut self,
        ty: Type,
        calling_convention: CallingConvention,
    ) -> Result<Option<X64Register>, CompileError> {
        let ret = match calling_convention {
            CallingConvention::WindowsFastcall => {
                static GPR_SEQ: &[GPR] = &[GPR::RCX, GPR::RDX, GPR::R8, GPR::R9];
                static XMM_SEQ: &[XMM] = &[XMM::XMM0, XMM::XMM1, XMM::XMM2, XMM::XMM3];
                let idx = self.n_gprs + self.n_xmms;
                match ty {
                    Type::I32 | Type::I64 => {
                        if idx < 4 {
                            let gpr = GPR_SEQ[idx];
                            self.n_gprs += 1;
                            Some(X64Register::GPR(gpr))
                        } else {
                            None
                        }
                    }
                    Type::F32 | Type::F64 => {
                        if idx < 4 {
                            let xmm = XMM_SEQ[idx];
                            self.n_xmms += 1;
                            Some(X64Register::XMM(xmm))
                        } else {
                            None
                        }
                    }
                    _ => {
                        return Err(CompileError::Codegen(format!(
                            "No register available for {calling_convention:?} and type {ty}"
                        )));
                    }
                }
            }
            _ => {
                static GPR_SEQ: &[GPR] =
                    &[GPR::RDI, GPR::RSI, GPR::RDX, GPR::RCX, GPR::R8, GPR::R9];
                static XMM_SEQ: &[XMM] = &[
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
                    _ => {
                        return Err(CompileError::Codegen(format!(
                            "No register available for {calling_convention:?} and type {ty}"
                        )));
                    }
                }
            }
        };

        Ok(ret)
    }
}

/// Create a new `MachineState` with default values.
pub fn new_machine_state() -> MachineState {
    MachineState {
        stack_values: vec![],
        register_values: vec![MachineValue::Undefined; 16 + 8],
        prev_frame: BTreeMap::new(),
        wasm_stack: vec![],
        wasm_inst_offset: usize::MAX,
    }
}
