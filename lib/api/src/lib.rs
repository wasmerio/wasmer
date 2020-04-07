#![deny(
    dead_code,
//    missing_docs,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
// Aspirational. I hope to have no unsafe code in this crate.
#![forbid(unsafe_code)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

//! TODO: Write high value, high-level API intro docs here
//! Intro/background information
//!
//! quick links to places in this document/other crates/standards etc.
//!
//! example code, link to projects using it
//!
//! more info, what to do if you run into problems

pub use crate::module::*;
pub use wasmer_runtime_core::instance::{DynFunc, Instance};
pub use wasmer_runtime_core::memory::Memory;
pub use wasmer_runtime_core::table::Table;
pub use wasmer_runtime_core::Func;
pub use wasmer_runtime_core::{func, imports};

pub mod module {
    //! Types and functions for WebAssembly modules.
    //!
    //! # Usage
    //! ## Create a Module
    //!
    //! ```
    //! ```
    //!
    //! ## Get the exports from a Module
    //! ```
    //! # use wasmer::*;
    //! # fn get_exports(module: &Module) {
    //! let exports: Vec<ExportDescriptor> = module.exports().collect();
    //! # }
    //! ```
    // TODO: verify that this is the type we want to export, with extra methods on it
    pub use wasmer_runtime_core::module::Module;
    // should this be in here?
    pub use wasmer_runtime_core::types::{ExportDescriptor, ExternDescriptor, ImportDescriptor};
    // TODO: implement abstract module API
}

pub mod memory {
    //! Types and functions for Wasm linear memory.
    pub use wasmer_runtime_core::memory::{Atomically, Memory, MemoryView};
}

pub mod wasm {
    //! Various types exposed by the Wasmer Runtime.
    //!
    //! TODO: Add index with links to sub sections
    //!
    //! # Globals
    //!
    //! # Tables
    pub use wasmer_runtime_core::global::Global;
    pub use wasmer_runtime_core::instance::Instance;
    pub use wasmer_runtime_core::memory::Memory;
    pub use wasmer_runtime_core::module::Module;
    pub use wasmer_runtime_core::table::Table;
    pub use wasmer_runtime_core::types::{ExportDescriptor, ExternDescriptor, ImportDescriptor};
    pub use wasmer_runtime_core::types::{
        FuncSig, GlobalDescriptor, MemoryDescriptor, TableDescriptor, Type, Value,
    };
    pub use wasmer_runtime_core::vm::Ctx;
}

pub mod import {
    //! Types and functions for Wasm imports.
    pub use wasmer_runtime_core::import::{ImportObject, ImportObjectIterator, Namespace};
    pub use wasmer_runtime_core::types::{ExternDescriptor, ImportDescriptor};
    pub use wasmer_runtime_core::{func, imports};
}

pub mod export {
    //! Types and functions for Wasm exports.
    pub use wasmer_runtime_core::types::{ExportDescriptor, ExternDescriptor};
}

pub mod units {
    //! Various unit types.
    pub use wasmer_runtime_core::units::{Bytes, Pages};
}

pub mod types {
    //! Types used in the Wasm runtime and conversion functions.
    pub use wasmer_runtime_core::types::{
        ElementType, FuncDescriptor, FuncSig, GlobalDescriptor, GlobalInit, MemoryDescriptor,
        TableDescriptor, Type, Value, ValueType,
    };
}

pub mod error {
    //! Various error types returned by Wasmer APIs.
    pub use wasmer_runtime_core::error::{CompileError, CompileResult};

    #[derive(Debug)]
    pub enum CompileFromFileError {
        CompileError(CompileError),
        IoError(std::io::Error),
    }

    impl std::fmt::Display for CompileFromFileError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                CompileFromFileError::CompileError(ce) => write!(f, "{}", ce),
                CompileFromFileError::IoError(ie) => write!(f, "{}", ie),
            }
        }
    }

    impl std::error::Error for CompileFromFileError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                CompileFromFileError::CompileError(ce) => Some(ce),
                CompileFromFileError::IoError(ie) => Some(ie),
            }
        }
    }

    impl From<CompileError> for CompileFromFileError {
        fn from(other: CompileError) -> Self {
            CompileFromFileError::CompileError(other)
        }
    }
    impl From<std::io::Error> for CompileFromFileError {
        fn from(other: std::io::Error) -> Self {
            CompileFromFileError::IoError(other)
        }
    }
}

/// Idea for generic trait; consider rename; it will need to be moved somewhere else
pub trait CompiledModule {
    fn new(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module>;
    fn from_binary(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module>;
    fn from_binary_unchecked(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module>;
    fn from_file(file: impl AsRef<std::path::Path>) -> Result<Module, error::CompileFromFileError>;

    fn validate(bytes: impl AsRef<[u8]>) -> error::CompileResult<()>;
}

use wasmer_runtime_core::backend::Compiler;

/// Copied from runtime core; TODO: figure out what we want to do here
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

    DefaultCompiler::new()
}

// this implementation should be moved
impl CompiledModule for Module {
    fn new(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module> {
        let bytes = bytes.as_ref();
        wasmer_runtime_core::compile_with(bytes, &default_compiler())
    }

    fn from_binary(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module> {
        let bytes = bytes.as_ref();
        wasmer_runtime_core::compile_with(bytes, &default_compiler())
    }

    fn from_binary_unchecked(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module> {
        // TODO: optimize this
        Self::from_binary(bytes)
    }

    fn from_file(file: impl AsRef<std::path::Path>) -> Result<Module, error::CompileFromFileError> {
        use std::fs;
        use std::io::Read;
        let path = file.as_ref();
        let mut f = fs::File::open(path)?;
        // TODO: ideally we can support a streaming compilation API and not have to read in the entire file
        let mut bytes = vec![];
        f.read_to_end(&mut bytes)?;

        Ok(Module::from_binary(bytes.as_slice())?)
    }

    fn validate(bytes: impl AsRef<[u8]>) -> error::CompileResult<()> {
        // TODO: optimize this
        let _ = Self::from_binary(bytes)?;
        Ok(())
    }
}

// Below this line is things copied from `wasmer-runtime` to make the C API work.
// All these additions should be reviewed carefully before shipping.

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
