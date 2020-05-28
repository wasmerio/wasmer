use std::error::Error;

pub use wasmer_runtime_core::export::Export;
pub use wasmer_runtime_core::global::Global;
pub use wasmer_runtime_core::import::{ImportObject, LikeNamespace};
pub use wasmer_runtime_core::instance::Instance;
pub use wasmer_runtime_core::memory::ptr::{Array, Item, WasmPtr};
pub use wasmer_runtime_core::memory::Memory;
pub use wasmer_runtime_core::module::Module;
pub use wasmer_runtime_core::table::Table;
pub use wasmer_runtime_core::types::Value;

pub use wasmer_runtime_core::import::imports;
pub use wasmer_runtime_core::typed_func::Func;
pub use wasmer_runtime_core::{compile, compile_with, validate};

pub mod memory {
    pub use wasmer_runtime_core::memory::{Atomically, Memory, MemoryView};
}

pub mod wasm {
    pub use wasmer_runtime_core::{
        global::Global,
        table::Table,
        types::{FuncSig, GlobalDescriptor, MemoryDescriptor, TableDescriptor, Type, Value},
    };
}

pub use wasmer_runtime_core::error::*;

pub mod units {
    pub use wasmer_runtime_core::units::{Bytes, Pages};
}

pub use wasmer_runtime_core::types::*;

/// Enum used to select which compiler should be used to generate code.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

pub enum InstantiateError {
    CompileError(Box<dyn Error>),
    InstantiationError(InstantiationError),
}

pub fn instantiate(
    wasm: &[u8],
    import_object: &ImportObject,
) -> Result<Instance, InstantiateError> {
    let module = compile(wasm).map_err(InstantiateError::CompileError)?;

    module
        .instantiate(import_object)
        .map_err(InstantiateError::InstantiationError)
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
