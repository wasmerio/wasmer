#[cfg(feature = "unwind")]
use gimli::{write::CallFrameInstruction, write::CommonInformationEntry, Encoding, Format, X86_64};
use std::fmt::Debug;
use wasmer_compiler::Architecture;

#[derive(Clone, Debug)]
pub enum UnwindOps {
    PushFP { up_to_sp: u32 },
    DefineNewFrame { up_to_sp: u32, down_to_clobber: u32 },
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
