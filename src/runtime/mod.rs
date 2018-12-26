mod backend;
mod backing;
mod instance;
mod memory;
mod module;
mod sig_registry;
mod table;
pub mod types;
pub mod vm;

pub use self::backend::{Compiler, FuncResolver};
pub use self::instance::{Import, Imports, Instance};
pub use self::module::{ItemName, Module, ModuleName};

/// Compile a webassembly module using the provided compiler and linked with the provided imports.
pub fn compile(
    compiler: &dyn Compiler,
    wasm: &[u8],
    imports: &Imports,
) -> Result<Box<Instance>, String> {
    let module = compiler.compile(wasm)?;
    Instance::new(module, imports)
}
