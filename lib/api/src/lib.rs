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

#[macro_use]
extern crate serde;

pub use crate::module::*;
pub use wasmer_runtime_core::instance::{DynFunc, Instance};
pub use wasmer_runtime_core::memory::Memory;
pub use wasmer_runtime_core::table::Table;
pub use wasmer_runtime_core::typed_func::DynamicFunc;
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
    //! let exports: Vec<ExportType> = module.exports().collect();
    //! # }
    //! ```
    // TODO: verify that this is the type we want to export, with extra methods on it
    pub use wasmer_runtime_core::module::Module;
    // should this be in here?
    pub use wasmer_runtime_core::types::{ExportType, ExternType, ImportType};
    // TODO: implement abstract module API
}

pub mod memory {
    //! Types and functions for Wasm linear memory.
    pub use wasmer_runtime_core::memory::{Atomically, Memory, MemoryView};
}

pub mod wasm {
    //! Various types exposed by the Wasmer Runtime relating to Wasm.
    //!
    //! TODO: Add index with links to sub sections
    //
    //! # Globals
    //!
    //! # Tables
    pub use wasmer_runtime_core::backend::Features;
    pub use wasmer_runtime_core::export::Export;
    pub use wasmer_runtime_core::global::Global;
    pub use wasmer_runtime_core::instance::{DynFunc, Instance};
    pub use wasmer_runtime_core::memory::Memory;
    pub use wasmer_runtime_core::module::Module;
    pub use wasmer_runtime_core::table::Table;
    pub use wasmer_runtime_core::types::{ExportType, ExternType, ImportType};
    pub use wasmer_runtime_core::types::{FuncSig, GlobalType, MemoryType, TableType, Type, Value};
    pub use wasmer_runtime_core::Func;
}

pub mod vm {
    //! Various types exposed by the Wasmer Runtime relating to the VM.
    pub use wasmer_runtime_core::vm::Ctx;
}

pub mod compiler {
    //! Types and functions for compiling wasm;
    use crate::module::Module;
    pub use wasmer_runtime_core::backend::{
        BackendCompilerConfig, Compiler, CompilerConfig, Features,
    };
    pub use wasmer_runtime_core::compile_with;
    #[cfg(unix)]
    pub use wasmer_runtime_core::fault::{pop_code_version, push_code_version};
    pub use wasmer_runtime_core::state::CodeVersion;

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
    pub fn compile(wasm: &[u8]) -> crate::error::CompileResult<Module> {
        wasmer_runtime_core::compile_with(&wasm[..], &default_compiler())
    }

    /// The same as `compile` but takes a `CompilerConfig` for the purpose of
    /// changing the compiler's behavior
    pub fn compile_with_config(
        wasm: &[u8],
        compiler_config: CompilerConfig,
    ) -> crate::error::CompileResult<Module> {
        wasmer_runtime_core::compile_with_config(&wasm[..], &default_compiler(), compiler_config)
    }

    /// The same as `compile_with_config` but takes a `Compiler` for the purpose of
    /// changing the backend.
    pub fn compile_with_config_with(
        wasm: &[u8],
        compiler_config: CompilerConfig,
        compiler: &dyn Compiler,
    ) -> crate::error::CompileResult<Module> {
        wasmer_runtime_core::compile_with_config(&wasm[..], compiler, compiler_config)
    }

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
            }
        }
    }
}

pub mod codegen {
    //! Types and functions for generating native code.
    pub use wasmer_runtime_core::codegen::ModuleCodeGenerator;
}

// TODO: `import` or `imports`?
pub mod import {
    //! Types and functions for Wasm imports.
    pub use wasmer_runtime_core::import::{
        ImportObject, ImportObjectIterator, LikeNamespace, Namespace,
    };
    pub use wasmer_runtime_core::types::{ExternType, ImportType};
    pub use wasmer_runtime_core::{func, imports};
}

pub mod export {
    //! Types and functions for Wasm exports.
    pub use wasmer_runtime_core::export::Export;
    pub use wasmer_runtime_core::types::{ExportType, ExternType};
}

pub mod units {
    //! Various unit types.
    pub use wasmer_runtime_core::units::{Bytes, Pages};
}

pub mod types {
    //! Types used in the Wasm runtime and conversion functions.
    pub use wasmer_runtime_core::types::{
        ElementType, FuncSig, FuncType, GlobalInit, GlobalType, MemoryType, TableType, Type, Value,
        ValueType,
    };
}

pub mod error {
    //! Various error types returned by Wasmer APIs.
    pub use wasmer_runtime_core::backend::ExceptionCode;
    pub use wasmer_runtime_core::error::{
        CallError, CompileError, CompileResult, CreationError, Error, LinkError, ResolveError,
        RuntimeError,
    };

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
    fn new_with_compiler(
        bytes: impl AsRef<[u8]>,
        compiler: Box<dyn compiler::Compiler>,
    ) -> error::CompileResult<Module>;
    fn from_binary(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module>;
    fn from_binary_unchecked(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module>;
    fn from_file(file: impl AsRef<std::path::Path>) -> Result<Module, error::CompileFromFileError>;

    fn validate(bytes: impl AsRef<[u8]>) -> error::CompileResult<()>;
}

// this implementation should be moved
impl CompiledModule for Module {
    fn new(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module> {
        let bytes = bytes.as_ref();
        wasmer_runtime_core::compile_with(bytes, &compiler::default_compiler())
    }

    fn new_with_compiler(
        bytes: impl AsRef<[u8]>,
        compiler: Box<dyn compiler::Compiler>,
    ) -> error::CompileResult<Module> {
        let bytes = bytes.as_ref();
        wasmer_runtime_core::compile_with(bytes, &*compiler)
    }

    fn from_binary(bytes: impl AsRef<[u8]>) -> error::CompileResult<Module> {
        let bytes = bytes.as_ref();
        wasmer_runtime_core::compile_with(bytes, &compiler::default_compiler())
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
