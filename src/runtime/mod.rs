
pub mod vm;
pub mod types;
mod backing;
mod memory;
mod backend;
mod module;
mod instance;
mod table;
mod sig_registry;

pub use self::backend::{Compiler, FuncResolver};
pub use self::instance::{Instance, Imports, Import};
pub use self::module::{ModuleName, ItemName, Module};

/// Compile a webassembly module using the provided compiler and linked with the provided imports.
pub fn compile(compiler: &dyn Compiler, wasm: &[u8], imports: &Imports) -> Result<Box<Instance>, String> {
    let module = compiler.compile(wasm)?;
    Instance::new(module, imports)
}