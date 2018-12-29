pub mod codegen;

use crate::runtime::{
    module::Module,
    backend::Compiler,
};
use std::sync::Arc;

struct Cranelift {}

impl Compiler for Cranelift {
    // Compiles towasm byte to module
    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String> {
        Ok(Arc::new(codegen::CraneliftModuleTrait::from_bytes(wasm)?))
    }
}
