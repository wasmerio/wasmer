use crate::{error::CompileResult, module::ModuleInner, types::LocalFuncIndex, vm};
use std::ptr::NonNull;

pub use crate::mmap::{Mmap, Protect};
pub use crate::sig_registry::SigRegistry;

pub trait Compiler {
    /// Compiles a `Module` from WebAssembly binary format
    fn compile(&self, wasm: &[u8]) -> CompileResult<ModuleInner>;
}

pub trait FuncResolver {
    fn get(
        &self,
        module: &ModuleInner,
        local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>>;
}
