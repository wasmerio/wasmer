use crate::abi::Architecture;
use itertools::Itertools;
use wasmer_compiler::abi::{PairSlot, ReturnAbi, ReturnSlot};
use wasmer_types::Type;

/// AMD64 System V return-value classification.
pub struct X86_64SystemV;

/// Two adjacent `F32`s share a vector register (SSE eightbyte); anything
/// else bit-packs into a raw integer register.
fn pair_slot(t0: Type, t1: Type) -> PairSlot {
    if t0 == Type::F32 && t1 == Type::F32 {
        PairSlot::F32Vector(t0, t1)
    } else {
        PairSlot::Raw(t0, t1)
    }
}

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
}

#[cfg(test)]
mod tests {
    use super::{Architecture, PairSlot, ReturnAbi, ReturnSlot, Type, X86_64SystemV};

    #[test]
    fn classify_x86_64_return_type_abi() {
        let classify_x86_64 = |types: &[Type]| X86_64SystemV.classify_return_type(types);

        assert_eq!(classify_x86_64(&[]), ReturnAbi::Void);
        assert_eq!(classify_x86_64(&[Type::I64]), ReturnAbi::Single(Type::I64));
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F64]),
            ReturnAbi::Pair(
                ReturnSlot::Natural(Type::I32),
                ReturnSlot::Natural(Type::F64)
            )
        );
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F32]),
            ReturnAbi::PackedPair(PairSlot::Raw(Type::I32, Type::F32))
        );
        assert_eq!(
            classify_x86_64(&[Type::F32, Type::F32, Type::I64]),
            ReturnAbi::PackedFirst(
                PairSlot::F32Vector(Type::F32, Type::F32),
                ReturnSlot::Natural(Type::I64)
            )
        );
        assert_eq!(
            classify_x86_64(&[Type::F64, Type::I32, Type::F32]),
            ReturnAbi::PackedLast(
                ReturnSlot::Natural(Type::F64),
                PairSlot::Raw(Type::I32, Type::F32)
            )
        );
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F32, Type::F32, Type::I32]),
            ReturnAbi::PackedQuads(
                PairSlot::Raw(Type::I32, Type::F32),
                PairSlot::Raw(Type::F32, Type::I32)
            )
        );
        assert_eq!(
            classify_x86_64(&[Type::V128, Type::I32]),
            ReturnAbi::Sret(vec![Type::V128, Type::I32])
        );
    }
}
