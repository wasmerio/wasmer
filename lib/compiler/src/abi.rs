//! Shared WebAssembly return-value ABI classification.

use crate::lib::std::vec::Vec;
use wasmer_types::Type;

/// How a single-register return slot is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnSlot {
    /// Carried in a register of its own natural type.
    Natural(Type),
    /// Carried as a raw same-width integer register, bypassing its natural
    /// register class (used on AArch64 where a float value only gets a dedicated
    /// vector register as part of a homogeneous floating-point aggregate).
    Raw(Type),
}

/// How two adjacent 32-bit return values share a single register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairSlot {
    /// Two `F32`s sharing a `<2 x float>` vector register.
    F32Vector(Type, Type),
    /// Two 32-bit values bit-concatenated into one raw integer register.
    Raw(Type, Type),
}

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
    Pair(ReturnSlot, ReturnSlot),
    /// Two 32-bit values packed into one register.
    PackedPair(PairSlot),
    /// The first two 32-bit values are packed into one register.
    PackedFirst(PairSlot, ReturnSlot),
    /// The last two 32-bit values are packed into one register.
    PackedLast(ReturnSlot, PairSlot),
    /// Four 32-bit values packed pairwise into two registers.
    PackedQuads(PairSlot, PairSlot),
    /// Values returned independently in an aggregate of registers.
    Unpacked(Vec<Type>),
    /// Values returned indirectly through a structure-return pointer.
    Sret(Vec<Type>),
}
