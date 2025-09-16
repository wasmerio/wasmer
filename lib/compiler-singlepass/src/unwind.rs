#[cfg(feature = "unwind")]
use gimli::write::{Address, CallFrameInstruction, CommonInformationEntry, FrameDescriptionEntry};
#[cfg(feature = "unwind")]
use gimli::{AArch64, Encoding, Format, X86_64};
use std::fmt::Debug;
#[cfg(feature = "unwind")]
use wasmer_types::target::Architecture;

use crate::location;

#[derive(Clone, Debug, Copy)]
pub enum UnwindRegister<R: location::Reg, S: location::Reg> {
    GPR(R),
    FPR(S),
}

impl<R: location::Reg, S: location::Reg> UnwindRegister<R, S> {
    pub(crate) fn to_dwarf(&self) -> gimli::Register {
        match self {
            Self::GPR(reg) => reg.to_dwarf(),
            Self::FPR(reg) => reg.to_dwarf(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum UnwindOps<R: location::Reg, S: location::Reg> {
    PushFP {
        up_to_sp: u32,
    },
    Push2Regs {
        reg1: UnwindRegister<R, S>,
        reg2: UnwindRegister<R, S>,
        up_to_sp: u32,
    },
    DefineNewFrame,
    SaveRegister {
        reg: UnwindRegister<R, S>,
        bp_neg_offset: i32,
    },
}

#[cfg(not(feature = "unwind"))]
pub type CallFrameInstruction = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnwindInstructions {
    pub instructions: Vec<(u32, CallFrameInstruction)>,
    pub len: u32,
}

#[cfg(feature = "unwind")]
pub enum UnwindFrame {
    SystemV(gimli::write::FrameDescriptionEntry),
}

#[cfg(not(feature = "unwind"))]
pub type UnwindFrame = u32;

#[cfg(feature = "unwind")]
impl UnwindInstructions {
    /// Converts the unwind information into a `FrameDescriptionEntry`.
    pub fn to_fde(&self, address: Address) -> UnwindFrame {
        let mut fde = FrameDescriptionEntry::new(address, self.len);
        for (offset, inst) in &self.instructions {
            fde.add_instruction(*offset, inst.clone());
        }
        UnwindFrame::SystemV(fde)
    }
}

/// generate a default systemv  cie
#[cfg(feature = "unwind")]
pub fn create_systemv_cie(arch: Architecture) -> Option<gimli::write::CommonInformationEntry> {
    match arch {
        Architecture::X86_64 => {
            let mut entry = CommonInformationEntry::new(
                Encoding {
                    address_size: 8,
                    format: Format::Dwarf32,
                    version: 1,
                },
                1,
                -8,
                X86_64::RA,
            );
            entry.add_instruction(CallFrameInstruction::Cfa(X86_64::RSP, 8));
            entry.add_instruction(CallFrameInstruction::Offset(X86_64::RA, -8));
            Some(entry)
        }
        Architecture::Aarch64(_) => {
            let mut entry = CommonInformationEntry::new(
                Encoding {
                    address_size: 8,
                    format: Format::Dwarf32,
                    version: 1,
                },
                1,
                -8,
                AArch64::X30,
            );
            entry.add_instruction(CallFrameInstruction::Cfa(AArch64::SP, 0));
            Some(entry)
        }
        _ => None,
    }
}
