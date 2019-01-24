pub use wasmer_runtime_core::import::ImportObject;
pub use wasmer_runtime_core::instance::{Function, Instance};
pub use wasmer_runtime_core::module::Module;
pub use wasmer_runtime_core::types::Value;
pub use wasmer_runtime_core::vm::Ctx;

pub use wasmer_runtime_core::{compile_with, validate};

pub use wasmer_runtime_core::error;
pub use wasmer_runtime_core::imports;

pub mod wasm {
    pub use wasmer_runtime_core::instance::Function;
    pub use wasmer_runtime_core::types::{FuncSig, Type, Value};
}

/// Compile WebAssembly binary code into a [`Module`].
/// This function is useful if it is necessary to
/// compile a module before it can be instantiated
/// (otherwise, the [`instantiate`] function should be used).
///
/// [`Module`]: struct.Module.html
/// [`instantiate`]: fn.instantiate.html
///
/// # Params:
/// * `wasm`: A `&[u8]` containing the
///   binary code of the wasm module you want to compile.
/// # Errors:
/// If the operation fails, the function returns `Err(error::CompileError::...)`.
#[cfg(feature = "wasmer-clif-backend")]
pub fn compile(wasm: &[u8]) -> error::CompileResult<Module> {
    use wasmer_clif_backend::CraneliftCompiler;
    wasmer_runtime_core::compile_with(&wasm[..], &CraneliftCompiler::new())
}

/// Compile and instantiate WebAssembly code without
/// creating a [`Module`].
///
/// [`Module`]: struct.Module.html
///
/// # Params:
/// * `wasm`: A `&[u8]` containing the
///   binary code of the wasm module you want to compile.
/// * `import_object`: An object containing the values to be imported
///   into the newly-created Instance, such as functions or
///   Memory objects. There must be one matching property
///   for each declared import of the compiled module or else a
///   LinkError is thrown.
/// # Errors:
/// If the operation fails, the function returns a
/// `error::CompileError`, `error::LinkError`, or
/// `error::RuntimeError` (all combined into an `error::Error`),
/// depending on the cause of the failure.
#[cfg(feature = "wasmer-clif-backend")]
pub fn instantiate(wasm: &[u8], import_object: ImportObject) -> error::Result<Instance> {
    let module = compile(wasm)?;
    module.instantiate(import_object)
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
