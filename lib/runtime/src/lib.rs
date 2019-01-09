#[cfg(test)]
#[macro_use]
extern crate field_offset;

#[macro_use]
mod macros;
mod backing;
mod instance;
mod memory;
mod recovery;
mod sighandler;
mod sig_registry;
mod mmap;
pub mod module;
pub mod backend;
pub mod table;
pub mod types;
pub mod vm;
pub mod vmcalls;

pub use self::instance::{Import, ImportResolver, Imports, FuncRef, Instance};
pub use self::memory::LinearMemory;

/// Compile a webassembly module using the provided compiler.
pub fn compile(wasm: &[u8], compiler: &dyn backend::Compiler) -> Result<module::Module, String> {
    compiler.compile(wasm)
}
