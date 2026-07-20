use crate::abi::Architecture;
use itertools::Itertools;
use wasmer_compiler::abi::{PairSlot, ReturnAbi, ReturnSlot};
use wasmer_types::Type;

/// AArch64 System V return-value classification.
pub struct Aarch64SystemV;

impl Architecture for Aarch64SystemV {
    /// Classifies AArch64 (AAPCS64) return values.
    ///
    /// A float value only gets its own vector register as part of a
    /// homogeneous floating-point aggregate (the `Unpacked` case below).
    fn classify_return_type(&self, types: &[Type]) -> ReturnAbi {
        if (2..=4).contains(&types.len())
            && (types.iter().all(|ty| [Type::F32, Type::F64].contains(ty)))
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

        let widths: Vec<_> = types
            .iter()
            .map(|ty| match ty {
                Type::I32 | Type::F32 | Type::ExceptionRef => 32,
                Type::I64 | Type::F64 | Type::ExternRef | Type::FuncRef => 64,
                Type::V128 => 128,
            })
            .collect_vec();

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
}
