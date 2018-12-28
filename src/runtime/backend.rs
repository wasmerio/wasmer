use crate::runtime::module::Module;
use std::sync::Arc;

pub trait Compiler {
    /// Compiles a `Module` from WebAssembly binary format
    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String>;
}
