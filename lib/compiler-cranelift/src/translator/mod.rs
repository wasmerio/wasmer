//! Tools for translating wasm function bytecode to Cranelift IR.

mod code_translator;
mod func_environ;
mod func_state;
mod func_translator;
mod translation_utils;
mod unwind;

pub use self::func_environ::{FuncEnvironment, GlobalVariable, TargetEnvironment};
pub use self::func_translator::FuncTranslator;
pub use self::translation_utils::{
    irlibcall_to_libcall, irreloc_to_relocationkind, signature_to_cranelift_ir,
};
#[cfg(feature = "unwind")]
pub(crate) use self::unwind::CraneliftUnwindInfo;
pub(crate) use self::unwind::compiled_function_unwind_info;

pub(crate) use {
    self::code_translator::EXN_REF_TYPE, self::code_translator::TAG_TYPE,
    self::func_state::LandingPad,
};
