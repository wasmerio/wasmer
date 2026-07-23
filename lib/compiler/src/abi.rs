//! Shared WebAssembly return-value ABI classification.

use crate::lib::std::vec::Vec;
use itertools::Itertools;
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

/// Two adjacent `F32`s share a vector register (SSE eightbyte); anything
/// else bit-packs into a raw integer register.
fn pair_slot(t0: Type, t1: Type) -> PairSlot {
    if t0 == Type::F32 && t1 == Type::F32 {
        PairSlot::F32Vector(t0, t1)
    } else {
        PairSlot::Raw(t0, t1)
    }
}

/// Classifies x86_64 return values.
pub fn classify_return_type_x86_64(types: &[Type]) -> ReturnAbi {
    let widths = types.iter().map(|ty| ty.bit_size(64)).collect_vec();

    match (types, widths.as_slice()) {
        ([], []) => ReturnAbi::Void,
        ([value], [_]) => ReturnAbi::Single(*value),
        ([first, second], [32, 64] | [64, 32] | [64, 64]) => {
            ReturnAbi::Pair(ReturnSlot::Natural(*first), ReturnSlot::Natural(*second))
        }
        ([first, second], [32, 32]) => ReturnAbi::PackedPair(pair_slot(*first, *second)),
        ([first, second, third], [32, 32, 32 | 64]) => {
            ReturnAbi::PackedFirst(pair_slot(*first, *second), ReturnSlot::Natural(*third))
        }
        ([first, second, third], [64, 32, 32]) => {
            ReturnAbi::PackedLast(ReturnSlot::Natural(*first), pair_slot(*second, *third))
        }
        ([first, second, third, fourth], [32, 32, 32, 32]) => {
            ReturnAbi::PackedQuads(pair_slot(*first, *second), pair_slot(*third, *fourth))
        }
        _ => ReturnAbi::Sret(types.to_vec()),
    }
}

/// Classifies AArch64 (AAPCS64) return values.
///
/// A float value only gets its own vector register as part of a
/// homogeneous floating-point aggregate (the `Unpacked` case below).
pub fn classify_return_type_aarch64(types: &[Type]) -> ReturnAbi {
    let widths = types.iter().map(|ty| ty.byte_size(64)).collect_vec();
    if (2..=4).contains(&types.len())
        && (types.iter().all(|ty| ty == &Type::F32) || types.iter().all(|ty| ty == &Type::F64))
    {
        return ReturnAbi::Unpacked(types.to_vec());
    }

    if let [first, second] = types
        && matches!(
            first,
            Type::I32 | Type::I64 | Type::F32 | Type::F64 | Type::ExceptionRef
        )
        && matches!(second, Type::FuncRef | Type::ExternRef)
    {
        return ReturnAbi::Pair(ReturnSlot::Raw(*first), ReturnSlot::Raw(*second));
    }

    match (types, widths.as_slice()) {
        ([], []) => ReturnAbi::Void,
        ([value], [_]) => ReturnAbi::Single(*value),
        ([first, second], [32, 64] | [64, 32] | [64, 64]) => {
            ReturnAbi::Pair(ReturnSlot::Raw(*first), ReturnSlot::Raw(*second))
        }
        ([first, second], [32, 32]) => ReturnAbi::PackedPair(PairSlot::Raw(*first, *second)),
        ([first, second, third], [32, 32, 32 | 64]) => {
            ReturnAbi::PackedFirst(PairSlot::Raw(*first, *second), ReturnSlot::Raw(*third))
        }
        ([first, second, third], [64, 32, 32]) => {
            ReturnAbi::PackedLast(ReturnSlot::Raw(*first), PairSlot::Raw(*second, *third))
        }
        ([first, second, third, fourth], [32, 32, 32, 32]) => ReturnAbi::PackedQuads(
            PairSlot::Raw(*first, *second),
            PairSlot::Raw(*third, *fourth),
        ),
        _ => ReturnAbi::Sret(types.to_vec()),
    }
}

/// Classifies LoongArch64 return values according to the LP64D psABI.
///
/// Note LoongArch64 uses the same aggregate rules as the RISC-V LP64D ABI.
pub fn classify_return_type_loongarch64(types: &[Type]) -> ReturnAbi {
    classify_return_type_riscv(types, true)
}

/// Classifies RISC-V return values according to the hard-float psABI.
pub fn classify_return_type_riscv(types: &[Type], is_riscv64: bool) -> ReturnAbi {
    let xlen = if is_riscv64 { 64 } else { 32 };
    let widths = types.iter().map(|ty| ty.bit_size(xlen)).collect_vec();

    // The hardware floating-point calling convention flattens only aggregates
    // with one or two fields. ABI_FLEN is 64 for the Linux *D ABIs used here,
    // so an F64 is eligible even on RV32.
    if let [first, second] = types {
        let is_float = |ty| matches!(ty, Type::F32 | Type::F64);
        let eligible = |ty, width| is_float(ty) || width <= xlen;
        if (is_float(*first) || is_float(*second))
            && eligible(*first, widths[0])
            && eligible(*second, widths[1])
        {
            return ReturnAbi::Pair(ReturnSlot::Natural(*first), ReturnSlot::Natural(*second));
        }
    }

    match (types, widths.as_slice()) {
        ([], []) => ReturnAbi::Void,
        ([value], [_]) => ReturnAbi::Single(*value),

        // RV64 integer-convention aggregates of at most two XLEN-sized chunks.
        ([first, second], [32, 64] | [64, 32] | [64, 64]) if is_riscv64 => {
            ReturnAbi::Pair(ReturnSlot::Raw(*first), ReturnSlot::Raw(*second))
        }
        ([first, second], [32, 32]) if is_riscv64 => {
            ReturnAbi::PackedPair(PairSlot::Raw(*first, *second))
        }
        ([first, second, third], [32, 32, 32 | 64]) if is_riscv64 => {
            ReturnAbi::PackedFirst(PairSlot::Raw(*first, *second), ReturnSlot::Raw(*third))
        }
        ([first, second, third], [64, 32, 32]) if is_riscv64 => {
            ReturnAbi::PackedLast(ReturnSlot::Raw(*first), PairSlot::Raw(*second, *third))
        }
        ([first, second, third, fourth], [32, 32, 32, 32]) if is_riscv64 => ReturnAbi::PackedQuads(
            PairSlot::Raw(*first, *second),
            PairSlot::Raw(*third, *fourth),
        ),
        // Two 32-bit fields fill the two integer return registers on RV32.
        ([first, second], [32, 32]) if !is_riscv64 => {
            ReturnAbi::PackedPair(PairSlot::Raw(*first, *second))
        }
        _ => ReturnAbi::Sret(types.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::classify_return_type_x86_64;
    use crate::abi::{PairSlot, ReturnAbi, ReturnSlot};
    use wasmer_types::Type;

    #[test]
    fn classify_x86_64_return_type_abi() {
        assert_eq!(classify_return_type_x86_64(&[]), ReturnAbi::Void);
        assert_eq!(
            classify_return_type_x86_64(&[Type::I64]),
            ReturnAbi::Single(Type::I64)
        );
        assert_eq!(
            classify_return_type_x86_64(&[Type::I32, Type::F64]),
            ReturnAbi::Pair(
                ReturnSlot::Natural(Type::I32),
                ReturnSlot::Natural(Type::F64)
            )
        );
        assert_eq!(
            classify_return_type_x86_64(&[Type::I32, Type::F32]),
            ReturnAbi::PackedPair(PairSlot::Raw(Type::I32, Type::F32))
        );
        assert_eq!(
            classify_return_type_x86_64(&[Type::F32, Type::F32, Type::I64]),
            ReturnAbi::PackedFirst(
                PairSlot::F32Vector(Type::F32, Type::F32),
                ReturnSlot::Natural(Type::I64)
            )
        );
        assert_eq!(
            classify_return_type_x86_64(&[Type::F64, Type::I32, Type::F32]),
            ReturnAbi::PackedLast(
                ReturnSlot::Natural(Type::F64),
                PairSlot::Raw(Type::I32, Type::F32)
            )
        );
        assert_eq!(
            classify_return_type_x86_64(&[Type::I32, Type::F32, Type::F32, Type::I32]),
            ReturnAbi::PackedQuads(
                PairSlot::Raw(Type::I32, Type::F32),
                PairSlot::Raw(Type::F32, Type::I32)
            )
        );
        assert_eq!(
            classify_return_type_x86_64(&[Type::V128, Type::I32]),
            ReturnAbi::Sret(vec![Type::V128, Type::I32])
        );
    }
}
