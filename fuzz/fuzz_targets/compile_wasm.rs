#![no_main]
#[macro_use]
extern crate libfuzzer_sys;
extern crate wasmer_runtime;
extern crate wasmer_runtime_core;
extern crate wasmer_llvm_backend;
extern crate wasmer_singlepass_backend;

use wasmer_runtime::{compile, compile_with};
use wasmer_runtime_core::backend::Compiler;

fn get_llvm_compiler() -> impl Compiler {
    use wasmer_llvm_backend::LLVMCompiler;
    LLVMCompiler::new()
}
fn get_singlepass_compiler() -> impl Compiler {
    use wasmer_singlepass_backend::SinglePassCompiler;
    SinglePassCompiler::new()
}

fuzz_target!(|data: &[u8]| {
    let _ = compile_with(data, &get_llvm_compiler());
    let _ = compile(data);
    let _ = compile_with(data, &get_singlepass_compiler());
});
