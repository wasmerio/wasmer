extern crate wasmer_clif_backend;
extern crate wasmer_runtime_core;

pub use wasmer_runtime_core::*;

use wasmer_clif_backend::CraneliftCompiler;

pub fn compile(wasm: &[u8]) -> error::CompileResult<module::Module> {
    wasmer_runtime_core::compile(&wasm[..], &CraneliftCompiler::new())
}
