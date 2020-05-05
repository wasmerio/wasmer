//! Trap codes describing the reason for a trap.

use core::fmt::{self, Display, Formatter};
use core::str::FromStr;
use serde::{Deserialize, Serialize};

/// A trap code describing the reason for a trap.
///
/// All trap instructions have an explicit trap code.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
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

    /// Table Elements doesn't fit the table size.
    ///
    /// This only can happen during instantiation.
    TableSetterOutOfBounds = 3,

    /// A `table_addr` instruction detected an out-of-bounds error.
    TableAccessOutOfBounds = 4,

    /// Other bounds checking error.
    OutOfBounds = 5,

    /// Indirect call to a null table entry.
    IndirectCallToNull = 6,

    /// Signature mismatch on indirect call.
    BadSignature = 7,

    /// An integer arithmetic operation caused an overflow.
    IntegerOverflow = 8,

    /// An integer division by zero.
    IntegerDivisionByZero = 9,

    /// Failed float-to-int conversion.
    BadConversionToInteger = 10,

    /// Code that was supposed to have been unreachable was reached.
    UnreachableCodeReached = 11,

    /// Execution has potentially run too long and may be interrupted.
    /// This trap is resumable.
    Interrupt = 12,
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
            Self::TableSetterOutOfBounds => "table out of bounds: elements segment does not fit",
            Self::TableAccessOutOfBounds => "undefined element: out of bounds table access",
            Self::OutOfBounds => "out of bounds",
            Self::IndirectCallToNull => "uninitialized element",
            Self::BadSignature => "indirect call type mismatch",
            Self::IntegerOverflow => "integer overflow",
            Self::IntegerDivisionByZero => "integer divide by zero",
            Self::BadConversionToInteger => "invalid conversion to integer",
            Self::UnreachableCodeReached => "unreachable",
            Self::Interrupt => "interrupt",
            // Self::User(_) => unreachable!(),
        }
    }
}

impl Display for TrapCode {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        use self::TrapCode::*;
        let identifier = match *self {
            StackOverflow => "stk_ovf",
            HeapSetterOutOfBounds => "heap_set_oob",
            HeapAccessOutOfBounds => "heap_get_oob",
            TableSetterOutOfBounds => "table_set_oob",
            TableAccessOutOfBounds => "table_get_oob",
            OutOfBounds => "oob",
            IndirectCallToNull => "icall_null",
            BadSignature => "bad_sig",
            IntegerOverflow => "int_ovf",
            IntegerDivisionByZero => "int_divz",
            BadConversionToInteger => "bad_toint",
            UnreachableCodeReached => "unreachable",
            Interrupt => "interrupt",
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
            // _ if s.starts_with("user") => s[4..].parse().map(User).map_err(|_| ()),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Everything but user-defined codes.
    const CODES: [TrapCode; 13] = [
        TrapCode::StackOverflow,
        TrapCode::HeapSetterOutOfBounds,
        TrapCode::HeapAccessOutOfBounds,
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
