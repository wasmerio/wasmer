//! RISC-V structures.

use crate::{
    common_decl::{MachineState, MachineValue, RegisterIndex},
    location::{CombinedRegister, Reg as AbstractReg},
};
use std::{collections::BTreeMap, slice::Iter};
use wasmer_types::target::CallingConvention;
use wasmer_types::{CompileError, Type};

/// General-purpose registers.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub enum GPR {
    // TODO: define integer registers X0-X31.
}

/// Floating-point registers.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[allow(clippy::upper_case_acronyms)]
pub enum FPR {
    // TODO: define floating-point registers F0-F31.
}

impl AbstractReg for GPR {
    fn is_callee_save(self) -> bool {
        // TODO: implement callee-save registers for RISC-V.
        todo!()
    }
    fn is_reserved(self) -> bool {
        // TODO: implement reserved registers for RISC-V (e.g., X0 always zero, stack pointer).
        todo!()
    }
    fn into_index(self) -> usize {
        self as usize
    }
    fn from_index(n: usize) -> Result<GPR, ()> {
        // TODO: map index to GPR.
        todo!()
    }
    fn iterator() -> Iter<'static, GPR> {
        // TODO: return an iterator over all GPR variants.
        todo!()
    }
    fn to_dwarf(self) -> u16 {
        // TODO: map register to DWARF register number.
        todo!()
    }
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
        register_values: vec![MachineValue::Undefined; /* GPR+FPR count */ 0],
        prev_frame: BTreeMap::new(),
        wasm_stack: vec![],
        wasm_inst_offset: usize::MAX,
    }
}