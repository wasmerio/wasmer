mod backend;
mod backing;
mod instance;
mod memory;
mod module;
mod sig_registry;
mod table;
pub mod types;
pub mod vm;
pub mod vmcalls;

pub use self::backend::Compiler;
pub use self::instance::{Import, Imports, Instance};
pub use self::module::Module;
pub use self::table::TableBacking;

/// Compile a webassembly module using the provided compiler and linked with the provided imports.
pub fn compile(
    compiler: &dyn Compiler,
    wasm: &[u8],
    imports: &Imports,
) -> Result<Box<Instance>, String> {
    let module = compiler.compile(wasm)?;
    Instance::new(module, imports)
}
