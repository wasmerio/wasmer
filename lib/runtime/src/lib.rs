#[macro_use]
mod macros;
mod backend;
mod backing;
mod instance;
mod memory;
pub mod mmap;
pub mod module;
mod recovery;
mod sig_registry;
mod sighandler;
mod table;
pub mod types;
pub mod vm;
pub mod vmcalls;

pub use self::backend::{Compiler, FuncResolver};
pub use self::instance::{Import, ImportResolver, Imports, FuncRef, Instance};
pub use self::memory::LinearMemory;
pub use self::module::{Module, ModuleInner};
pub use self::sig_registry::SigRegistry;

/// Compile a webassembly module using the provided compiler.
pub fn compile(wasm: &[u8], compiler: &dyn Compiler) -> Result<Module, String> {
    compiler.compile(wasm)
}
