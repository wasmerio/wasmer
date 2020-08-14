//! A jump table is a method of transferring program control (branching)
//! to another part of a program (or a different program that may have
//! been dynamically loaded) using a table of branch or jump instructions.
//!
//! Source: https://en.wikipedia.org/wiki/Branch_table

use super::CodeOffset;
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_types::entity::{entity_impl, SecondaryMap};

/// An opaque reference to a [jump table](https://en.wikipedia.org/wiki/Branch_table).
///
/// `JumpTable`s are used for indirect branching and are specialized for dense,
/// 0-based jump offsets.
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JumpTable(u32);

entity_impl!(JumpTable, "jt");

impl JumpTable {
    /// Create a new jump table reference from its number.
    ///
    /// This method is for use by the parser.
    pub fn with_number(n: u32) -> Option<Self> {
        if n < u32::max_value() {
            Some(Self(n))
        } else {
            None
        }
    }
}

/// Code offsets for Jump Tables.
pub type JumpTableOffsets = SecondaryMap<JumpTable, CodeOffset>;
