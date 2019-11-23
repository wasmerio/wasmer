pub use wabt::wat2wasm;
use wasmer_llvm_backend::LLVMCompiler;
use wasmer_runtime_core::backend::Compiler;

pub fn get_compiler() -> impl Compiler {
    LLVMCompiler::new()
}
