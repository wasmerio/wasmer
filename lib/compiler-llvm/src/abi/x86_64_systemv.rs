use crate::abi::Architecture;
use itertools::Itertools;
use wasmer_compiler::abi::ReturnAbi;
use wasmer_types::Type;

/// AMD64 System V return-value classification.
pub struct X86_64SystemV;

impl Architecture for X86_64SystemV {
    fn classify_return_type(&self, types: &[Type]) -> ReturnAbi {
        let widths: Vec<_> = types
            .iter()
            .map(|ty| match ty {
                Type::I32 | Type::F32 | Type::ExceptionRef => 32,
                Type::I64 | Type::F64 => 64,
                Type::ExternRef | Type::FuncRef => 64,
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

#[cfg(test)]
mod tests {
    use super::{Architecture, ReturnAbi, Type, X86_64SystemV};

    #[test]
    fn classify_x86_64_return_type_abi() {
        let classify_x86_64 = |types: &[Type]| X86_64SystemV.classify_return_type(types);

        assert_eq!(classify_x86_64(&[]), ReturnAbi::Void);
        assert_eq!(classify_x86_64(&[Type::I64]), ReturnAbi::Single(Type::I64));
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F64]),
            ReturnAbi::Pair(Type::I32, Type::F64)
        );
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F32]),
            ReturnAbi::PackedPair(Type::I32, Type::F32)
        );
        assert_eq!(
            classify_x86_64(&[Type::F32, Type::F32, Type::I64]),
            ReturnAbi::PackedFirst(Type::F32, Type::F32, Type::I64)
        );
        assert_eq!(
            classify_x86_64(&[Type::F64, Type::I32, Type::F32]),
            ReturnAbi::PackedLast(Type::F64, Type::I32, Type::F32)
        );
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F32, Type::F32, Type::I32]),
            ReturnAbi::PackedQuads(Type::I32, Type::F32, Type::F32, Type::I32)
        );
        assert_eq!(
            classify_x86_64(&[Type::V128, Type::I32]),
            ReturnAbi::Sret(vec![Type::V128, Type::I32])
        );
    }
}
