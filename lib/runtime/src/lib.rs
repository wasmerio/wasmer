#![deny(unused_imports, unused_variables, unused_unsafe, unreachable_patterns)]

//! Wasmer-runtime is a library that makes embedding WebAssembly
//! in your application easy, efficient, and safe.
//!
//! # How to use Wasmer-Runtime
//!
//! The easiest way is to use the [`instantiate`] function to create an [`Instance`].
//! Then you can use [`call`] or [`func`] and then [`call`][func.call] to call an exported function safely.
//!
//! [`instantiate`]: fn.instantiate.html
//! [`Instance`]: struct.Instance.html
//! [`call`]: struct.Instance.html#method.call
//! [`func`]: struct.Instance.html#method.func
//! [func.call]: struct.Function.html#method.call
//!
//! ## Here's an example:
//!
//! Given this WebAssembly:
//!
//! ```wat
//! (module
//!   (type $t0 (func (param i32) (result i32)))
//!   (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
//!     get_local $p0
//!     i32.const 1
//!     i32.add))
//! ```
//!
//! compiled into wasm bytecode, we can call the exported "add_one" function:
//!
//! ```
//! static WASM: &'static [u8] = &[
//!     // The module above compiled to bytecode goes here.
//!     // ...
//! #   0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60,
//! #   0x01, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07,
//! #   0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x00, 0x00, 0x0a, 0x09, 0x01,
//! #   0x07, 0x00, 0x20, 0x00, 0x41, 0x01, 0x6a, 0x0b, 0x00, 0x1a, 0x04, 0x6e,
//! #   0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07, 0x61, 0x64, 0x64, 0x5f,
//! #   0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02, 0x70, 0x30,
//! ];
//!
//! use wasmer_runtime::{
//!     instantiate,
//!     Value,
//!     imports,
//!     error,
//!     Func,
//! };
//!
//! fn main() -> error::Result<()> {
//!     // We're not importing anything, so make an empty import object.
//!     let import_object = imports! {};
//!
//!     let mut instance = instantiate(WASM, &import_object)?;
//!
//!     let add_one: Func<i32, i32> = instance.func("add_one")?;
//!
//!     let value = add_one.call(42)?;
//!
//!     assert_eq!(value, 43);
//!     
//!     Ok(())
//! }
//! ```
//!
//! # Additional Notes:
//!
//! The `wasmer-runtime` is build to support compiler multiple backends.
//! Currently, we support the [Cranelift] compiler with the [`wasmer-clif-backend`] crate.
//!
//! You can specify the compiler you wish to use with the [`compile_with`] function.
//!
//! [Cranelift]: https://github.com/CraneStation/cranelift
//! [`wasmer-clif-backend`]: https://crates.io/crates/wasmer-clif-backend
//! [`compile_with`]: fn.compile_with.html

pub use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
pub use wasmer_runtime_core::export::Export;
pub use wasmer_runtime_core::global::Global;
pub use wasmer_runtime_core::import::ImportObject;
pub use wasmer_runtime_core::instance::{DynFunc, Instance};
pub use wasmer_runtime_core::memory::Memory;
pub use wasmer_runtime_core::module::Module;
pub use wasmer_runtime_core::table::Table;
pub use wasmer_runtime_core::types::Value;
pub use wasmer_runtime_core::vm::Ctx;

pub use wasmer_runtime_core::Func;
pub use wasmer_runtime_core::{compile_with, validate};
pub use wasmer_runtime_core::{func, imports};

pub mod memory {
    pub use wasmer_runtime_core::memory::{Atomic, Atomically, Memory, MemoryView};
}

pub mod wasm {
    //! Various types exposed by the Wasmer Runtime.
    pub use wasmer_runtime_core::global::Global;
    pub use wasmer_runtime_core::table::Table;
    pub use wasmer_runtime_core::types::{
        FuncSig, GlobalDescriptor, MemoryDescriptor, TableDescriptor, Type, Value,
    };
}

pub mod error {
    pub use wasmer_runtime_core::cache::Error as CacheError;
    pub use wasmer_runtime_core::error::*;
}

pub mod units {
    //! Various unit types.
    pub use wasmer_runtime_core::units::{Bytes, Pages};
}

pub mod cache;

use wasmer_runtime_core::backend::{Compiler, CompilerConfig};

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
pub fn compile(wasm: &[u8]) -> error::CompileResult<Module> {
    wasmer_runtime_core::compile_with(&wasm[..], &default_compiler())
}

/// The same as `compile` but takes a `CompilerConfig` for the purpose of
/// changing the compiler's behavior
pub fn compile_with_config(
    wasm: &[u8],
    compiler_config: CompilerConfig,
) -> error::CompileResult<Module> {
    wasmer_runtime_core::compile_with_config(&wasm[..], &default_compiler(), compiler_config)
}

/// The same as `compile_with_config` but takes a `Compiler` for the purpose of
/// changing the backend.
pub fn compile_with_config_with(
    wasm: &[u8],
    compiler_config: CompilerConfig,
    compiler: &dyn Compiler,
) -> error::CompileResult<Module> {
    wasmer_runtime_core::compile_with_config(&wasm[..], compiler, compiler_config)
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
pub fn instantiate(wasm: &[u8], import_object: &ImportObject) -> error::Result<Instance> {
    let module = compile(wasm)?;
    module.instantiate(import_object)
}

/// Get a single instance of the default compiler to use.
pub fn default_compiler() -> impl Compiler {
    #[cfg(feature = "llvm")]
    use wasmer_llvm_backend::LLVMCompiler as DefaultCompiler;

    #[cfg(feature = "singlepass")]
    use wasmer_singlepass_backend::SinglePassCompiler as DefaultCompiler;

    #[cfg(not(any(feature = "llvm", feature = "singlepass")))]
    use wasmer_clif_backend::CraneliftCompiler as DefaultCompiler;

    DefaultCompiler::new()
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
