use crate::{
    backend::Backend, cache::Artifact, get_global_store, module::Module, new,
    renew_global_store_with,
};
use std::{convert::Infallible, error::Error};

pub use new::wasmer::wat2wasm;

/// Compile WebAssembly binary code into a [`Module`].
/// This function is useful if it is necessary to
/// compile a module before it can be instantiated
/// (otherwise, the [`instantiate`] function should be used).
///
/// [`Module`]: struct.Module.html
/// [`instantiate`]: fn.instantiate.html
///
/// # Params
///
/// * `wasm`: A `&[u8]` containing the
///   binary code of the wasm module you want to compile.
///
/// # Errors
///
/// If the operation fails, the function returns `Err(error::CompileError::...)`.
///
/// This function only exists if one of `default-backend-llvm`, `default-backend-cranelift`,
/// or `default-backend-singlepass` is set.
pub fn compile(bytes: &[u8]) -> Result<Module, Box<dyn Error>> {
    compile_with(bytes, Backend::Auto)
}

/// Creates a new module from the given cache [`Artifact`]
pub fn load_cache_with(cache: Artifact) -> Result<Module, Infallible> {
    Ok(cache.module())
}

/// Compile a [`Module`] using the provided compiler from
/// WebAssembly binary code. This function is useful if it
/// is necessary to a compile a module before it can be instantiated
/// and must be used if you wish to use a different backend from the default.
///
/// # Note
///
/// This second parameter aren't used any more in the deprecated
/// version of `wasmer-runtime-core`.
pub fn compile_with(bytes: &[u8], compiler: Backend) -> Result<Module, Box<dyn Error>> {
    renew_global_store_with(compiler);

    Ok(Module::new(new::wasmer::Module::new(
        &get_global_store(),
        bytes,
    )?))
}

/// The same as `compile_with` but changes the compiler behavior
/// with the values in the `CompilerConfig`
///
/// # Note
///
/// This second and third parameters aren't used any more in the
/// deprecated version of `wasmer-runtime-core`.
pub fn compile_with_config(
    bytes: &[u8],
    _compiler: (),
    _compiler_config: (),
) -> Result<Module, Box<dyn Error>> {
    Ok(Module::new(new::wasmer::Module::new(
        &get_global_store(),
        bytes,
    )?))
}

/// Perform validation as defined by the
/// WebAssembly specification. Returns `true` if validation
/// succeeded, `false` if validation failed.
pub fn validate(bytes: &[u8]) -> bool {
    new::wasmer::Module::validate(&get_global_store(), bytes).is_ok()
}

/// Helper macro to create a new `Func` object using the provided function pointer.
///
/// # Usage
///
/// Function pointers or closures are supported. Closures can capture
/// their environment (with `move`). The first parameter is of kind
/// `vm::Ctx`.
///
/// ```
/// # use wasmer_runtime_core::{imports, func, vm};
///
/// // A host function.
/// fn func(ctx: &mut vm::Ctx, n: i32) -> i32 {
///     n
/// }
///
/// let i = 7;
///
/// let import_object = imports! {
///     "env" => {
///         "foo" => func!(func),
///         // A closure with a captured environment.
///         "bar" => func!(move |_: &mut vm::Ctx, n: i32| -> i32 {
///             n + i
///         }),
///     },
/// };
/// ```
#[macro_export]
macro_rules! func {
    ($function:expr) => {
        $crate::typed_func::Func::new($function)
    };
}
