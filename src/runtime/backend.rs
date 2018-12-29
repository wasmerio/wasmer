use crate::runtime::{
    module::Module,
    types::FuncIndex,
    vm,
};
use std::ptr::NonNull;
use std::sync::Arc;

pub trait Compiler {
    /// Compiles a `Module` from WebAssembly binary format
    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String>;
}

pub trait FuncResolver {
    fn get(&self, module: &Module, index: FuncIndex) -> Option<NonNull<vm::Func>>;
}