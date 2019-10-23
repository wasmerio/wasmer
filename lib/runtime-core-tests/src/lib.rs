pub use wabt::wat2wasm;
use wasmer_runtime_core::backend::Compiler;

#[cfg(feature = "backend-cranelift")]
pub fn get_compiler() -> impl Compiler {
    use wasmer_clif_backend::CraneliftCompiler;

    CraneliftCompiler::new()
}

#[cfg(feature = "backend-singlepass")]
pub fn get_compiler() -> impl Compiler {
    use wasmer_singlepass_backend::SinglePassCompiler;
    SinglePassCompiler::new()
}

#[cfg(feature = "backend-llvm")]
pub fn get_compiler() -> impl Compiler {
    use wasmer_llvm_backend::LLVMCompiler;
    LLVMCompiler::new()
}
