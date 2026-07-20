//! Shared WebAssembly return-value ABI classification.

use crate::lib::std::vec::Vec;
use wasmer_types::Type;

/// Describes how a list of WebAssembly values is returned by a native ABI.
///
/// Every non-void variant retains the values it classified so signature
/// construction, packing, and unpacking all use the same ABI rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnAbi {
    /// No return values.
    Void,
    /// One value returned directly.
    Single(Type),
    /// Two values returned independently in registers.
    Pair(Type, Type),
    /// Two 32-bit values packed into one register.
    PackedPair(Type, Type),
    /// The first two 32-bit values are packed into one register.
    PackedFirst(Type, Type, Type),
    /// The last two 32-bit values are packed into one register.
    PackedLast(Type, Type, Type),
    /// Four 32-bit values packed pairwise into two registers.
    PackedQuads(Type, Type, Type, Type),
    /// Values returned independently in an aggregate of registers.
    Unpacked(Vec<Type>),
    /// Values returned indirectly through a structure-return pointer.
    Sret(Vec<Type>),
}
