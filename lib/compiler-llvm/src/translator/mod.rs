mod code;
pub mod intrinsics;
mod state;

pub use self::code::FuncTranslator;

pub(crate) use code::LLVMIR_LARGE_FUNCTION_THRESHOLD;
