use crate::{module::Module, types::FuncIndex, vm};
use std::ptr::NonNull;

pub trait Compiler {
    /// Compiles a `Module` from WebAssembly binary format
    fn compile(&self, wasm: &[u8]) -> Result<Module, String>;
}

pub trait FuncResolver {
    fn get(&self, module: &Module, index: FuncIndex) -> Option<NonNull<vm::Func>>;
}
