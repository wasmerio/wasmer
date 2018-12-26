use crate::runtime::{module::Module, types::FuncIndex, vm};
use std::ptr::NonNull;
use std::sync::Arc;

pub trait Compiler {
    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String>;
}

pub trait FuncResolver {
    fn resolve(&self, index: FuncIndex) -> Option<NonNull<vm::Func>>;
}
