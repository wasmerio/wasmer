mod backing;
mod instance;
mod memory;
mod sig_registry;
mod table;
pub mod module;
pub mod backend;
pub mod types;
pub mod vm;
pub mod vmcalls;

pub use self::backend::{Compiler, FuncResolver};
pub use self::instance::{Import, ImportResolver, Instance};
pub use self::module::Module;

/// Compile a webassembly module using the provided compiler and linked with the provided imports.
pub fn instantiate(
    wasm: &[u8],
    compiler: &dyn Compiler,
    imports: &dyn ImportResolver,
) -> Result<Box<Instance>, String> {
    let module = compiler.compile(wasm)?;
    Instance::new(module, imports)
}
