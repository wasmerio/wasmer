#[cfg(feature = "wasmer-clif-backend")]
extern crate wasmer_clif_backend;

extern crate wasmer_runtime_core;

pub use wasmer_runtime_core::*;

#[cfg(feature = "wasmer-clif-backend")]
use wasmer_clif_backend::CraneliftCompiler;

/// Compiles a WebAssembly module
pub fn compile(wasm: &[u8]) -> error::CompileResult<module::Module> {
    #[cfg(not(feature = "wasmer-clif-backend"))]
    panic!("compile/1 is not available when default compiler is disabled");
    #[cfg(feature = "wasmer-clif-backend")]
    wasmer_runtime_core::compile(&wasm[..], &CraneliftCompiler::new())
}
