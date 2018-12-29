pub mod codegen;

use crate::runtime::{
    module::Module,
    backend::Compiler,
};

use std::sync::Arc;

use self::codegen::{
    CraneliftModule,
    converter,
};

pub struct Cranelift {}

impl Compiler for Cranelift {
    // Compiles towasm byte to module
    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String> {
        // Generate a Cranlift module from wasm binary
        let cranelift_module = CraneliftModule::from_bytes(wasm)?;

        // Convert Cranelift module to wasmer module
        let wasmer_module = converter::generate_wasmer_module(cranelift_module);

        // Return new wasmer module
        Ok(Arc::new(wasmer_module))
    }
}
