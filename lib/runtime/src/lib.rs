#![deny(
    dead_code,
    missing_docs,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

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
//! ```ignore
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
//!     let add_one: Func<i32, i32> = instance.exports.get("add_one")?;
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
//! `wasmer-runtime` is built to support multiple compiler backends.
//! Currently, we support the Singlepass, [Cranelift], and LLVM compilers
//! with the [`wasmer-singlepass-backend`], [`wasmer-clif-backend`], and
//! wasmer-llvm-backend crates, respectively.
//!
//! You can specify the compiler you wish to use with the [`compile_with`]
//! function or use the default with the [`compile`] function.
//!
//! [Cranelift]: https://github.com/CraneStation/cranelift
//! [LLVM]: https://llvm.org
//! [`wasmer-singlepass-backend`]: https://crates.io/crates/wasmer-singlepass-backend
//! [`wasmer-clif-backend`]: https://crates.io/crates/wasmer-clif-backend

#[macro_use]
extern crate serde_derive;

pub use wasmer_runtime_core::backend::{ExceptionCode, Features};
pub use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
pub use wasmer_runtime_core::export::Export;
pub use wasmer_runtime_core::global::Global;
pub use wasmer_runtime_core::import::{ImportObject, LikeNamespace};
pub use wasmer_runtime_core::instance::{DynFunc, Instance};
pub use wasmer_runtime_core::memory::ptr::{Array, Item, WasmPtr};
pub use wasmer_runtime_core::memory::Memory;
pub use wasmer_runtime_core::module::Module;
pub use wasmer_runtime_core::table::Table;
pub use wasmer_runtime_core::types::Value;
pub use wasmer_runtime_core::vm::Ctx;

pub use wasmer_runtime_core::Func;
pub use wasmer_runtime_core::{compile_with, validate};
pub use wasmer_runtime_core::{func, imports};

#[cfg(unix)]
pub use wasmer_runtime_core::{
    fault::{pop_code_version, push_code_version},
    state::CodeVersion,
};

pub mod memory {
    //! The memory module contains the implementation data structures and helper functions used to
    //! manipulate and access wasm memory.
    pub use wasmer_runtime_core::memory::{Atomically, Memory, MemoryView};
}

pub mod wasm {
    //! Various types exposed by the Wasmer Runtime.
    pub use wasmer_runtime_core::global::Global;
    pub use wasmer_runtime_core::table::Table;
    pub use wasmer_runtime_core::types::{FuncSig, GlobalType, MemoryType, TableType, Type, Value};
}

pub mod error {
    //! The error module contains the data structures and helper functions used to implement errors that
    //! are produced and returned from the wasmer runtime.
    pub use wasmer_runtime_core::cache::Error as CacheError;
    pub use wasmer_runtime_core::error::*;
}

pub mod units {
    //! Various unit types.
    pub use wasmer_runtime_core::units::{Bytes, Pages};
}

pub mod types {
    //! Types used in the Wasm runtime and conversion functions.
    pub use wasmer_runtime_core::types::*;
}

pub mod cache;

pub use wasmer_runtime_core::backend::{Compiler, CompilerConfig};

/// Enum used to select which compiler should be used to generate code.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub enum Backend {
    #[cfg(feature = "singlepass")]
    /// Singlepass backend
    Singlepass,
    #[cfg(feature = "cranelift")]
    /// Cranelift backend
    Cranelift,
    #[cfg(feature = "llvm")]
    /// LLVM backend
    LLVM,
    /// Auto backend
    Auto,
}

impl Backend {
    /// Get a list of the currently enabled (via feature flag) backends.
    pub fn variants() -> &'static [&'static str] {
        &[
            #[cfg(feature = "singlepass")]
            "singlepass",
            #[cfg(feature = "cranelift")]
            "cranelift",
            #[cfg(feature = "llvm")]
            "llvm",
            "auto",
        ]
    }

    /// Stable string representation of the backend.
    /// It can be used as part of a cache key, for example.
    pub fn to_string(&self) -> &'static str {
        match self {
            #[cfg(feature = "singlepass")]
            Backend::Singlepass => "singlepass",
            #[cfg(feature = "cranelift")]
            Backend::Cranelift => "cranelift",
            #[cfg(feature = "llvm")]
            Backend::LLVM => "llvm",
            Backend::Auto => "auto",
        }
    }
}

impl Default for Backend {
    fn default() -> Self {
        #[cfg(all(feature = "default-backend-singlepass", not(feature = "docs")))]
        return Backend::Singlepass;

        #[cfg(any(feature = "default-backend-cranelift", feature = "docs"))]
        return Backend::Cranelift;

        #[cfg(all(feature = "default-backend-llvm", not(feature = "docs")))]
        return Backend::LLVM;

        #[cfg(not(any(
            feature = "default-backend-singlepass",
            feature = "default-backend-cranelift",
            feature = "default-backend-llvm",
        )))]
        panic!("There is no default-backend set.");
    }
}

impl std::str::FromStr for Backend {
    type Err = String;
    fn from_str(s: &str) -> Result<Backend, String> {
        match s.to_lowercase().as_str() {
            #[cfg(feature = "singlepass")]
            "singlepass" => Ok(Backend::Singlepass),
            #[cfg(feature = "cranelift")]
            "cranelift" => Ok(Backend::Cranelift),
            #[cfg(feature = "llvm")]
            "llvm" => Ok(Backend::LLVM),
            "auto" => Ok(Backend::Auto),
            _ => Err(format!("The backend {} doesn't exist", s)),
        }
    }
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
///
/// This function only exists if one of `default-backend-llvm`, `default-backend-cranelift`,
/// or `default-backend-singlepass` is set.
#[cfg(any(
    feature = "default-backend-singlepass",
    feature = "default-backend-cranelift",
    feature = "default-backend-llvm",
))]
pub fn compile(wasm: &[u8]) -> error::CompileResult<Module> {
    wasmer_runtime_core::compile_with(&wasm[..], &default_compiler())
}

/// The same as `compile` but takes a `CompilerConfig` for the purpose of
/// changing the compiler's behavior
///
/// This function only exists if one of `default-backend-llvm`, `default-backend-cranelift`,
/// or `default-backend-singlepass` is set.
#[cfg(any(
    feature = "default-backend-singlepass",
    feature = "default-backend-cranelift",
    feature = "default-backend-llvm",
))]
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
///
/// This function only exists if one of `default-backend-llvm`, `default-backend-cranelift`,
/// or `default-backend-singlepass` is set.
#[cfg(any(
    feature = "default-backend-singlepass",
    feature = "default-backend-cranelift",
    feature = "default-backend-llvm",
))]
pub fn instantiate(wasm: &[u8], import_object: &ImportObject) -> error::Result<Instance> {
    let module = compile(wasm)?;
    module.instantiate(import_object)
}

/// Get a single instance of the default compiler to use.
///
/// The output of this function can be controlled by the mutually
/// exclusive `default-backend-llvm`, `default-backend-singlepass`,
/// and `default-backend-cranelift` feature flags.
///
/// This function only exists if one of `default-backend-llvm`, `default-backend-cranelift`,
/// or `default-backend-singlepass` is set.
#[cfg(any(
    feature = "default-backend-singlepass",
    feature = "default-backend-cranelift",
    feature = "default-backend-llvm",
))]
pub fn default_compiler() -> impl Compiler {
    #[cfg(any(
        all(
            feature = "default-backend-llvm",
            not(feature = "docs"),
            any(
                feature = "default-backend-cranelift",
                feature = "default-backend-singlepass"
            )
        ),
        all(
            not(feature = "docs"),
            feature = "default-backend-cranelift",
            feature = "default-backend-singlepass"
        )
    ))]
    compile_error!(
        "The `default-backend-X` features are mutually exclusive.  Please choose just one"
    );

    #[cfg(all(feature = "default-backend-llvm", not(feature = "docs")))]
    use wasmer_llvm_backend::LLVMCompiler as DefaultCompiler;

    #[cfg(all(feature = "default-backend-singlepass", not(feature = "docs")))]
    use wasmer_singlepass_backend::SinglePassCompiler as DefaultCompiler;

    #[cfg(any(feature = "default-backend-cranelift", feature = "docs"))]
    use wasmer_clif_backend::CraneliftCompiler as DefaultCompiler;

    return DefaultCompiler::new();
}

/// Get the `Compiler` as a trait object for the given `Backend`.
/// Returns `Option` because support for the requested `Compiler` may
/// not be enabled by feature flags.
///
/// To get a list of the enabled backends as strings, call `Backend::variants()`.
pub fn compiler_for_backend(backend: Backend) -> Option<Box<dyn Compiler>> {
    match backend {
        #[cfg(feature = "cranelift")]
        Backend::Cranelift => Some(Box::new(wasmer_clif_backend::CraneliftCompiler::new())),

        #[cfg(any(feature = "singlepass"))]
        Backend::Singlepass => Some(Box::new(
            wasmer_singlepass_backend::SinglePassCompiler::new(),
        )),

        #[cfg(feature = "llvm")]
        Backend::LLVM => Some(Box::new(wasmer_llvm_backend::LLVMCompiler::new())),

        Backend::Auto => {
            #[cfg(feature = "default-backend-singlepass")]
            return Some(Box::new(
                wasmer_singlepass_backend::SinglePassCompiler::new(),
            ));
            #[cfg(feature = "default-backend-cranelift")]
            return Some(Box::new(wasmer_clif_backend::CraneliftCompiler::new()));
            #[cfg(feature = "default-backend-llvm")]
            return Some(Box::new(wasmer_llvm_backend::LLVMCompiler::new()));

            #[cfg(not(any(
                feature = "default-backend-singlepass",
                feature = "default-backend-cranelift",
                feature = "default-backend-llvm",
            )))]
            panic!("There is no default-compiler set.");
        }
    }
}

/// The current version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod test {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn str_repr_matches() {
        // if this test breaks, think hard about why it's breaking
        // can we avoid having these be different?

        for &backend in &[
            #[cfg(feature = "cranelift")]
            Backend::Cranelift,
            #[cfg(feature = "llvm")]
            Backend::LLVM,
            #[cfg(feature = "singlepass")]
            Backend::Singlepass,
        ] {
            assert_eq!(backend, Backend::from_str(backend.to_string()).unwrap());
        }
    }
}
