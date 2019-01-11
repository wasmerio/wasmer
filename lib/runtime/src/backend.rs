use crate::{module::Module, types::FuncIndex, vm};
use std::ptr::NonNull;

pub use crate::mmap::{Mmap, Protect};
pub use crate::sig_registry::SigRegistry;

pub trait Compiler {
    /// Compiles a `Module` from WebAssembly binary format
    fn compile(&self, wasm: &[u8]) -> Result<Module, String>;
}

pub trait FuncResolver {
    fn get(&self, module: &Module, index: FuncIndex) -> Option<NonNull<vm::Func>>;
}
