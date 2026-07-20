use crate::abi::Architecture;
use itertools::Itertools;
use wasmer_compiler::abi::{PairSlot, ReturnAbi, ReturnSlot};
use wasmer_types::Type;

/// RISC-V System V return-value classification.
pub(crate) struct RiscvSystemV {
    pub(crate) is_riscv64: bool,
}

/// Two adjacent `F32`s share a vector register; anything else bit-packs
/// into a raw integer register.
fn pair_slot(t0: Type, t1: Type) -> PairSlot {
    if t0 == Type::F32 && t1 == Type::F32 {
        PairSlot::F32Vector(t0, t1)
    } else {
        PairSlot::Raw(t0, t1)
    }
}

impl Architecture for RiscvSystemV {
    fn classify_return_type(&self, types: &[Type]) -> ReturnAbi {
        let widths: Vec<_> = types
            .iter()
            .map(|ty| match ty {
                Type::I32 | Type::F32 | Type::ExceptionRef => 32,
                Type::I64 | Type::F64 => 64,
                Type::ExternRef | Type::FuncRef => {
                    if self.is_riscv64 {
                        64
                    } else {
                        32
                    }
                }
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
