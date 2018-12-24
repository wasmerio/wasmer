use crate::runtime::module::Module;
use crate::runtime::types::FuncIndex;
use crate::runtime::{
    vm,
    module::Module,
    types::FuncIndex,
};

pub trait Compiler {
    fn compile(wasm: &[u8]) -> Box<Module>;
}

pub trait FuncResolver {
    pub fn resolve(&self, index: FuncIndex) -> *const vm::Func;
}