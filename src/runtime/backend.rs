use crate::runtime::{module::Module, vm};
use std::ptr::NonNull;
use std::sync::Arc;

pub trait Compiler {
    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String>;
}

pub trait FuncResolver {
    fn resolve(&self, module: &Module, name: &str) -> Option<NonNull<vm::Func>>;
}
