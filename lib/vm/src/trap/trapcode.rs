// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! Trap codes describing the reason for a trap.

use core::fmt::{self, Display, Formatter};
use core::str::FromStr;
use loupe::MemoryUsage;
#[cfg(feature = "enable-rkyv")]
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A trap code describing the reason for a trap.
///
/// All trap instructions have an explicit trap code.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize, Error, MemoryUsage)]
#[cfg_attr(
    feature = "enable-rkyv",
    derive(RkyvSerialize, RkyvDeserialize, Archive)
)]
#[repr(u32)]
pub enum TrapCode {
    /// The current stack space was exhausted.
    ///
    /// On some platforms, a stack overflow may also be indicated by a segmentation fault from the
    /// stack guard page.
    StackOverflow = 0,

    /// Memory data doesn't fit the memory size.
    ///
    /// This only can happen during instantiation.
    HeapSetterOutOfBounds = 1,

    /// A `heap_addr` instruction detected an out-of-bounds error.
    ///
    /// Note that not all out-of-bounds heap accesses are reported this way;
    /// some are detected by a segmentation fault on the heap unmapped or
    /// offset-guard pages.
    HeapAccessOutOfBounds = 2,

    /// A `heap_addr` instruction was misaligned.
    HeapMisaligned = 3,

    /// Table Elements doesn't fit the table size.
    ///
    /// This only can happen during instantiation.
    TableSetterOutOfBounds = 4,

    /// A `table_addr` instruction detected an out-of-bounds error.
    TableAccessOutOfBounds = 5,

    /// Other bounds checking error.
    OutOfBounds = 6,

    /// Indirect call to a null table entry.
    IndirectCallToNull = 7,

    /// Signature mismatch on indirect call.
    BadSignature = 8,

    /// An integer arithmetic operation caused an overflow.
    IntegerOverflow = 9,

    /// An integer division by zero.
    IntegerDivisionByZero = 10,

    /// Failed float-to-int conversion.
    BadConversionToInteger = 11,

    /// Code that was supposed to have been unreachable was reached.
    UnreachableCodeReached = 12,

    /// Execution has potentially run too long and may be interrupted.
    /// This trap is resumable.
    Interrupt = 13,

    /// An atomic memory access was attempted with an unaligned pointer.
    UnalignedAtomic = 14,

    /// A trap indicating that the runtime was unable to allocate sufficient memory.
    VMOutOfMemory = 15,
    // /// A user-defined trap code.
    // User(u16),
}

impl TrapCode {
    /// Gets the message for this trap code
    pub fn message(&self) -> &str {
        match self {
            Self::StackOverflow => "call stack exhausted",
            Self::HeapSetterOutOfBounds => "memory out of bounds: data segment does not fit",
            Self::HeapAccessOutOfBounds => "out of bounds memory access",
            Self::HeapMisaligned => "misaligned heap",
            Self::TableSetterOutOfBounds => {
                "out of bounds table access: elements segment does not fit"
            }
            Self::TableAccessOutOfBounds => "undefined element: out of bounds table access",
            Self::OutOfBounds => "out of bounds",
            Self::IndirectCallToNull => "uninitialized element",
            Self::BadSignature => "indirect call type mismatch",
            Self::IntegerOverflow => "integer overflow",
            Self::IntegerDivisionByZero => "integer divide by zero",
            Self::BadConversionToInteger => "invalid conversion to integer",
            Self::UnreachableCodeReached => "unreachable",
            Self::Interrupt => "interrupt",
            Self::UnalignedAtomic => "unaligned atomic access",
            Self::VMOutOfMemory => "out of memory",
            // Self::User(_) => unreachable!(),
        }
    }
}

impl Display for TrapCode {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let identifier = match *self {
            Self::StackOverflow => "stk_ovf",
            Self::HeapSetterOutOfBounds => "heap_set_oob",
            Self::HeapAccessOutOfBounds => "heap_get_oob",
            Self::HeapMisaligned => "heap_misaligned",
            Self::TableSetterOutOfBounds => "table_set_oob",
            Self::TableAccessOutOfBounds => "table_get_oob",
            Self::OutOfBounds => "oob",
            Self::IndirectCallToNull => "icall_null",
            Self::BadSignature => "bad_sig",
            Self::IntegerOverflow => "int_ovf",
            Self::IntegerDivisionByZero => "int_divz",
            Self::BadConversionToInteger => "bad_toint",
            Self::UnreachableCodeReached => "unreachable",
            Self::Interrupt => "interrupt",
            Self::UnalignedAtomic => "unalign_atom",
            Self::VMOutOfMemory => "oom",
            // User(x) => return write!(f, "user{}", x),
        };
        f.write_str(identifier)
    }
}

impl FromStr for TrapCode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use self::TrapCode::*;
        match s {
            "stk_ovf" => Ok(StackOverflow),
            "heap_set_oob" => Ok(HeapSetterOutOfBounds),
            "heap_get_oob" => Ok(HeapAccessOutOfBounds),
            "heap_misaligned" => Ok(HeapMisaligned),
            "table_set_oob" => Ok(TableSetterOutOfBounds),
            "table_get_oob" => Ok(TableAccessOutOfBounds),
            "oob" => Ok(OutOfBounds),
            "icall_null" => Ok(IndirectCallToNull),
            "bad_sig" => Ok(BadSignature),
            "int_ovf" => Ok(IntegerOverflow),
            "int_divz" => Ok(IntegerDivisionByZero),
            "bad_toint" => Ok(BadConversionToInteger),
            "unreachable" => Ok(UnreachableCodeReached),
            "interrupt" => Ok(Interrupt),
            "unalign_atom" => Ok(UnalignedAtomic),
            "oom" => Ok(VMOutOfMemory),
            // _ if s.starts_with("user") => s[4..].parse().map(User).map_err(|_| ()),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Everything but user-defined codes.
    const CODES: [TrapCode; 15] = [
        TrapCode::StackOverflow,
        TrapCode::HeapSetterOutOfBounds,
        TrapCode::HeapAccessOutOfBounds,
        TrapCode::HeapMisaligned,
        TrapCode::TableSetterOutOfBounds,
        TrapCode::TableAccessOutOfBounds,
        TrapCode::OutOfBounds,
        TrapCode::IndirectCallToNull,
        TrapCode::BadSignature,
        TrapCode::IntegerOverflow,
        TrapCode::IntegerDivisionByZero,
        TrapCode::BadConversionToInteger,
        TrapCode::UnreachableCodeReached,
        TrapCode::Interrupt,
        TrapCode::UnalignedAtomic,
    ];

    #[test]
    fn display() {
        for r in &CODES {
            let tc = *r;
            assert_eq!(tc.to_string().parse(), Ok(tc));
        }
        assert_eq!("bogus".parse::<TrapCode>(), Err(()));

        // assert_eq!(TrapCode::User(17).to_string(), "user17");
        // assert_eq!("user22".parse(), Ok(TrapCode::User(22)));
        assert_eq!("user".parse::<TrapCode>(), Err(()));
        assert_eq!("user-1".parse::<TrapCode>(), Err(()));
        assert_eq!("users".parse::<TrapCode>(), Err(()));
    }
}
