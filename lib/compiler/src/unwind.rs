//! A `CompiledFunctionUnwindInfo` contains the function unwind information.
//!
//! The unwind information is used to determine which function
//! called the function that threw the exception, and which
//! function called that one, and so forth.
//!
//! More info: https://en.wikipedia.org/wiki/Call_stack
use crate::std::vec::Vec;
use crate::{Addend, CodeOffset};
use serde::{Deserialize, Serialize};

/// Relocation Entry data
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FDERelocEntry(pub i64, pub usize, pub u8);

/// Relocation entry for unwind info.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FunctionTableReloc {
    /// Entry offest in the code block.
    pub offset: CodeOffset,
    /// Entry addend relative to the code block.
    pub addend: Addend,
}

/// Compiled function unwind information.
///
/// > Note: Windows OS have a different way of representing the [unwind info],
/// > That's why we keep the Windows data and the Unix frame layout in different
/// > fields.
///
/// [unwind info]: https://docs.microsoft.com/en-us/cpp/build/exception-handling-x64?view=vs-2019
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum CompiledFunctionUnwindInfo {
    /// Windows UNWIND_INFO.
    Windows(Vec<u8>),

    /// Unix frame layout info.
    FrameLayout(Vec<u8>, usize, Vec<FDERelocEntry>),
}

impl CompiledFunctionUnwindInfo {
    /// Retuns true is no unwind info data.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Windows(d) => d.is_empty(),
            Self::FrameLayout(c, _, _) => c.is_empty(),
        }
    }

    /// Returns size of serilized unwind info.
    pub fn len(&self) -> usize {
        match self {
            Self::Windows(d) => d.len(),
            Self::FrameLayout(c, _, _) => c.len(),
        }
    }

    /// Serializes data into byte array.
    pub fn serialize(&self, dest: &mut [u8], relocs: &mut Vec<FunctionTableReloc>) {
        match self {
            Self::Windows(d) => {
                dest.copy_from_slice(d);
            }
            Self::FrameLayout(code, _fde_offset, r) => {
                dest.copy_from_slice(code);
                r.iter().for_each(move |r| {
                    assert_eq!(r.2, 8);
                    relocs.push(FunctionTableReloc {
                        offset: r.1 as _,
                        addend: r.0,
                    })
                });
            }
        }
    }
}
