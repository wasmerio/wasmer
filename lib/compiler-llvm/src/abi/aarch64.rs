use crate::abi::Architecture;
use itertools::Itertools;
use wasmer_compiler::abi::ReturnAbi;
use wasmer_types::Type;

/// AArch64 System V return-value classification.
pub struct Aarch64SystemV;

impl Architecture for Aarch64SystemV {
    fn classify_return_type(&self, types: &[Type]) -> ReturnAbi {
        if (2..=4).contains(&types.len())
            && (types.iter().all(|ty| *ty == Type::F32) || types.iter().all(|ty| *ty == Type::F64))
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
            return ReturnAbi::Pair(*first, *second);
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
            ([first, second], [32, 64] | [64, 32] | [64, 64]) => ReturnAbi::Pair(*first, *second),
            ([first, second], [32, 32]) => ReturnAbi::PackedPair(*first, *second),
            ([first, second, third], [32, 32, 32 | 64]) => {
                ReturnAbi::PackedFirst(*first, *second, *third)
            }
            ([first, second, third], [64, 32, 32]) => {
                ReturnAbi::PackedLast(*first, *second, *third)
            }
            ([first, second, third, fourth], [32, 32, 32, 32]) => {
                ReturnAbi::PackedQuads(*first, *second, *third, *fourth)
            }
            _ => ReturnAbi::Sret(types.to_vec()),
        }
    }
}
