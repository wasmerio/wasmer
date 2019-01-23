#[doc(inline)]
pub use wasmer_runtime_core::*;

pub use wasmer_runtime_core::instance::Instance;
pub use wasmer_runtime_core::module::Module;
pub use wasmer_runtime_core::validate;

/// The `compile(...)` function compiles a `Module`
/// from WebAssembly binary code. This function is useful if it
/// is necessary to a compile a module before it can be instantiated
/// (otherwise, the webassembly::instantiate() function should be used).
///
/// Params:
/// * `wasm`: A `&[u8]` containing the
///   binary code of the wasm module you want to compile.
/// Errors:
/// If the operation fails, the function returns `Err(error::CompileError::...).`
#[cfg(feature = "wasmer-clif-backend")]
pub fn compile(wasm: &[u8]) -> error::CompileResult<module::Module> {
    use wasmer_clif_backend::CraneliftCompiler;
    wasmer_runtime_core::compile_with(&wasm[..], &CraneliftCompiler::new())
}

/// The `instantiate(...)` function allows you to compile and
/// instantiate WebAssembly code in one go.
///
/// Params:
/// * `wasm`: A `&[u8]` containing the
///   binary code of the wasm module you want to compile.
/// * `import_object`: An object containing the values to be imported
///   into the newly-created Instance, such as functions or
///   webassembly::Memory objects. There must be one matching property
///   for each declared import of the compiled module or else a
///   webassembly::LinkError is thrown.
/// Errors:
/// If the operation fails, the function returns a
/// `error::CompileError`, `error::LinkError`, or
/// `error::RuntimeError` (all combined into an `error::Error`),
/// depending on the cause of the failure.
#[cfg(feature = "wasmer-clif-backend")]
pub fn instantiate(
    wasm: &[u8],
    import_object: import::ImportObject,
) -> error::Result<instance::Instance> {
    let module = compile(wasm)?;
    module.instantiate(import_object)
}
