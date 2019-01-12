#[cfg(test)]
#[macro_use]
extern crate field_offset;

#[macro_use]
mod macros;
mod backing;
mod instance;
mod mmap;
mod recovery;
mod sig_registry;
mod sighandler;
pub mod backend;
pub mod export;
pub mod import;
pub mod module;
pub mod memory;
pub mod table;
pub mod types;
pub mod vm;
pub mod vmcalls;

pub use self::instance::Instance;

/// Compile a webassembly module using the provided compiler.
pub fn compile(wasm: &[u8], compiler: &dyn backend::Compiler) -> Result<module::Module, String> {
    compiler.compile(wasm)
}
