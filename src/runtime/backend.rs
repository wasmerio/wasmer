use crate::runtime::module::Module;
use crate::runtime::types::FuncIndex;
use crate::runtime::{
    vm,
    module::Module,
    types::FuncIndex,
};
use std::sync::Arc;

pub trait Compiler {
    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String>;
}

pub trait FuncResolver {
    pub fn resolve(&self, index: FuncIndex) -> Option<*const vm::Func>;
}