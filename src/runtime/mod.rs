pub mod backend;
mod backing;
mod instance;
pub mod memory;
pub mod module;
mod sig_registry;
mod table;
pub mod types;
pub mod vm;
pub mod vmcalls;

pub use self::backend::{Compiler, FuncResolver};
pub use self::instance::{Import, ImportResolver, Imports, Instance, InstanceABI, InstanceOptions};
pub use self::module::Module;
pub use self::table::TableBacking;
pub use self::sig_registry::SigRegistry;

/// Compile a webassembly module using the provided compiler and linked with the provided imports.
pub fn instantiate(
    wasm: &[u8],
    compiler: &dyn Compiler,
    imports: &dyn ImportResolver,
) -> Result<Box<Instance>, String> {
    let module = compiler.compile(wasm)?;
    Instance::new(module, imports)
}
