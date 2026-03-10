#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]

pub use wabt::wat2wasm;
use wasmer_llvm_backend::LLVMCompiler;
use wasmer_runtime_core::backend::Compiler;

pub fn get_compiler() -> impl Compiler {
    LLVMCompiler::new()
}
