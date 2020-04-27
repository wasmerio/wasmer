use std::collections::HashMap;
use std::fmt;

/// An exception table for a `RunnableModule`.
#[derive(Clone, Debug, Default)]
pub struct ExceptionTable {
    /// Mappings from offsets in generated machine code to the corresponding exception code.
    pub offset_to_code: HashMap<usize, ExceptionCode>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ExceptionCode {
    /// An `unreachable` opcode was executed.
    Unreachable = 0,
    /// Call indirect incorrect signature trap.
    IncorrectCallIndirectSignature = 1,
    /// Memory out of bounds trap.
    MemoryOutOfBounds = 2,
    /// Call indirect out of bounds trap.
    CallIndirectOOB = 3,
    /// An arithmetic exception, e.g. divided by zero.
    IllegalArithmetic = 4,
    /// Misaligned atomic access trap.
    MisalignedAtomicAccess = 5,
}

impl fmt::Display for ExceptionCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ExceptionCode::Unreachable => "unreachable",
                ExceptionCode::IncorrectCallIndirectSignature => {
                    "incorrect `call_indirect` signature"
                }
                ExceptionCode::MemoryOutOfBounds => "memory out-of-bounds access",
                ExceptionCode::CallIndirectOOB => "`call_indirect` out-of-bounds",
                ExceptionCode::IllegalArithmetic => "illegal arithmetic operation",
                ExceptionCode::MisalignedAtomicAccess => "misaligned atomic access",
            }
        )
    }
}