#[cfg(feature = "unwind")]
use gimli::write::{Address, CallFrameInstruction, CommonInformationEntry, FrameDescriptionEntry};
#[cfg(feature = "unwind")]
use gimli::{Encoding, Format, X86_64};
use std::fmt::Debug;
#[cfg(feature = "unwind")]
use wasmer_compiler::Architecture;

#[derive(Clone, Debug)]
pub enum UnwindOps {
    PushFP { up_to_sp: u32 },
    DefineNewFrame,
    SaveRegister { reg: u16, bp_neg_offset: i32 },
}

#[cfg(not(feature = "unwind"))]
pub type CallFrameInstruction = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnwindInstructions {
    pub instructions: Vec<(u32, CallFrameInstruction)>,
    pub len: u32,
}

impl UnwindInstructions {
    /// Converts the unwind information into a `FrameDescriptionEntry`.
    pub fn to_fde(&self, address: Address) -> gimli::write::FrameDescriptionEntry {
        let mut fde = FrameDescriptionEntry::new(address, self.len);
        for (offset, inst) in &self.instructions {
            fde.add_instruction(*offset, inst.clone().into());
        }
        fde
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
        Architecture::Aarch64(_) => None,
        _ => None,
    }
}
