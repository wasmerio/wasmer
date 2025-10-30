use crate::machine::*;
use std::fmt::Debug;
use std::hash::Hash;
use std::slice::Iter;

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Multiplier {
    Zero = 0,
    One = 1,
    Two = 2,
    Four = 4,
    Height = 8,
}

#[allow(dead_code, clippy::upper_case_acronyms)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Location<R, S> {
    GPR(R),
    SIMD(S),
    Memory(R, i32),
    Memory2(R, R, Multiplier, i32), // R + R*Multiplier + i32
    Imm8(u8),
    Imm32(u32),
    Imm64(u64),
    None,
}

impl<R, S> MaybeImmediate for Location<R, S> {
    fn imm_value(&self) -> Option<Value> {
        match *self {
            Location::Imm8(imm) => Some(Value::I8(imm as i8)),
            Location::Imm32(imm) => Some(Value::I32(imm as i32)),
            Location::Imm64(imm) => Some(Value::I64(imm as i64)),
            _ => None,
        }
    }
}

#[allow(unused)]
pub trait Reg: Copy + Clone + Eq + PartialEq + Debug + Hash + Ord {
    fn into_index(self) -> usize;
    fn from_index(i: usize) -> Result<Self, ()>;
    fn iterator() -> Iter<'static, Self>;

    #[cfg(feature = "unwind")]
    fn to_dwarf(self) -> gimli::Register;
}

#[allow(unused)]
pub trait Descriptor<R: Reg, S: Reg> {
    const FP: R;
    const VMCTX: R;
    const GPR_COUNT: usize;
    const SIMD_COUNT: usize;
    const WORD_SIZE: usize;
    const STACK_GROWS_DOWN: bool;
    const FP_STACK_ARG_OFFSET: i32;
    const ARG_REG_COUNT: usize;
    fn callee_save_gprs() -> Vec<R>;
    fn caller_save_gprs() -> Vec<R>;
    fn callee_save_simd() -> Vec<S>;
    fn caller_save_simd() -> Vec<S>;
    fn callee_param_location(n: usize) -> Location<R, S>;
    fn caller_arg_location(n: usize) -> Location<R, S>;
    fn return_location() -> Location<R, S>;
}

#[allow(unused)]
pub trait CombinedRegister: Copy + Clone + Eq + PartialEq + Debug {
    /// Convert from a GPR register
    fn from_gpr(x: u16) -> Self;
    /// Convert from an SIMD register
    fn from_simd(x: u16) -> Self;
}
