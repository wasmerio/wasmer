#[cfg(test)]
#[macro_use]
extern crate field_offset;

#[macro_use]
mod macros;
#[doc(hidden)]
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
pub mod structures;
pub mod table;
pub mod types;
pub mod vm;
#[doc(hidden)]
pub mod vmcalls;

pub use self::import::Imports;
pub use self::instance::Instance;
#[doc(inline)]
pub use self::module::Module;
use std::rc::Rc;

/// Compile a webassembly module using the provided compiler.
pub fn compile(wasm: &[u8], compiler: &dyn backend::Compiler) -> Result<module::Module, String> {
    compiler
        .compile(wasm)
        .map(|inner| module::Module::new(Rc::new(inner)))
}
