#[cfg(test)]
#[macro_use]
extern crate field_offset;

#[macro_use]
mod macros;
pub mod backend;
mod backing;
pub mod export;
pub mod import;
mod instance;
pub mod memory;
mod mmap;
pub mod module;
mod recovery;
mod sig_registry;
mod sighandler;
pub mod table;
pub mod types;
pub mod vm;
pub mod vmcalls;

pub use self::instance::Instance;

/// Compile a webassembly module using the provided compiler.
pub fn compile(wasm: &[u8], compiler: &dyn backend::Compiler) -> Result<module::Module, String> {
    compiler.compile(wasm)
}
