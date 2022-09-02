#[cfg(feature = "core")]
use crate::alloc::borrow::Cow;
use crate::js::lib::std::string::String;
use crate::js::trap::RuntimeError;
#[cfg(feature = "std")]
use std::borrow::Cow;
#[cfg(feature = "std")]
use thiserror::Error;
use wasmer_types::ImportError;

// Compilation Errors
//
// If `std` feature is enable, we can't use `thiserror` until
// https://github.com/dtolnay/thiserror/pull/64 is merged.

/// The WebAssembly.CompileError object indicates an error during
/// WebAssembly decoding or validation.
///
/// This is based on the [Wasm Compile Error][compile-error] API.
///
/// [compiler-error]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/CompileError
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum CompileError {
    /// A Wasm translation error occured.
    #[cfg_attr(feature = "std", error("WebAssembly translation error: {0}"))]
    Wasm(WasmError),

    /// A compilation error occured.
    #[cfg_attr(feature = "std", error("Compilation error: {0}"))]
    Codegen(String),

    /// The module did not pass validation.
    #[cfg_attr(feature = "std", error("Validation error: {0}"))]
    Validate(String),

    /// The compiler doesn't support a Wasm feature
    #[cfg_attr(feature = "std", error("Feature {0} is not yet supported"))]
    UnsupportedFeature(String),

    /// The compiler cannot compile for the given target.
    /// This can refer to the OS, the chipset or any other aspect of the target system.
    #[cfg_attr(feature = "std", error("The target {0} is not yet supported (see https://docs.wasmer.io/ecosystem/wasmer/wasmer-features)"))]
    UnsupportedTarget(String),

    /// Insufficient resources available for execution.
    #[cfg_attr(feature = "std", error("Insufficient resources: {0}"))]
    Resource(String),
}

#[cfg(feature = "core")]
impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CompileError")
    }
}

impl From<WasmError> for CompileError {
    fn from(original: WasmError) -> Self {
        Self::Wasm(original)
    }
}

/// A WebAssembly translation error.
///
/// When a WebAssembly function can't be translated, one of these error codes will be returned
/// to describe the failure.
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum WasmError {
    /// The input WebAssembly code is invalid.
    ///
    /// This error code is used by a WebAssembly translator when it encounters invalid WebAssembly
    /// code. This should never happen for validated WebAssembly code.
    #[cfg_attr(
        feature = "std",
        error("Invalid input WebAssembly code at offset {offset}: {message}")
    )]
    InvalidWebAssembly {
        /// A string describing the validation error.
        message: String,
        /// The bytecode offset where the error occurred.
        offset: usize,
    },

    /// A feature used by the WebAssembly code is not supported by the embedding environment.
    ///
    /// Embedding environments may have their own limitations and feature restrictions.
    #[cfg_attr(feature = "std", error("Unsupported feature: {0}"))]
    Unsupported(String),

    /// A Javascript value could not be converted to the requested type.
    #[cfg_attr(feature = "std", error("{0} doesn't match js value type {1}"))]
    TypeMismatch(Cow<'static, str>, Cow<'static, str>),

    /// A generic error.
    #[cfg_attr(feature = "std", error("{0}"))]
    Generic(String),
}

impl From<wasm_bindgen::JsValue> for WasmError {
    fn from(err: wasm_bindgen::JsValue) -> Self {
        Self::Generic(
            if err.is_string() && err.as_string().filter(|s| !s.is_empty()).is_some() {
                err.as_string().unwrap_or_default()
            } else {
                format!("Unexpected Javascript error: {:?}", err)
            },
        )
    }
}

/// The Serialize error can occur when serializing a
/// compiled Module into a binary.
/// Copied from wasmer_compiler::SerializeError
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum SerializeError {
    /// An IO error
    #[cfg_attr(feature = "std", error(transparent))]
    Io(#[cfg_attr(feature = "std", from)] std::io::Error),
    /// A generic serialization error
    #[cfg_attr(feature = "std", error("{0}"))]
    Generic(String),
}

/// The Deserialize error can occur when loading a
/// compiled Module from a binary.
/// Copied from wasmer_compiler::DeSerializeError
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum DeserializeError {
    /// An IO error
    #[cfg_attr(feature = "std", error(transparent))]
    Io(#[cfg_attr(feature = "std", from)] std::io::Error),
    /// A generic deserialization error
    #[cfg_attr(feature = "std", error("{0}"))]
    Generic(String),
    /// Incompatible serialized binary
    #[cfg_attr(feature = "std", error("incompatible binary: {0}"))]
    Incompatible(String),
    /// The provided binary is corrupted
    #[cfg_attr(feature = "std", error("corrupted binary: {0}"))]
    CorruptedBinary(String),
    /// The binary was valid, but we got an error when
    /// trying to allocate the required resources.
    #[cfg_attr(feature = "std", error(transparent))]
    Compiler(CompileError),
}

/// The WebAssembly.LinkError object indicates an error during
/// module instantiation (besides traps from the start function).
///
/// This is based on the [link error][link-error] API.
///
/// [link-error]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/LinkError
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
#[cfg_attr(feature = "std", error("Link error: {0}"))]
pub enum LinkError {
    /// An error occurred when checking the import types.
    #[cfg_attr(feature = "std", error("Error while importing {0:?}.{1:?}: {2}"))]
    Import(String, String, ImportError),

    #[cfg(not(target_arch = "wasm32"))]
    /// A trap ocurred during linking.
    #[cfg_attr(feature = "std", error("RuntimeError occurred during linking: {0}"))]
    Trap(#[source] RuntimeError),
    /// Insufficient resources available for linking.
    #[cfg_attr(feature = "std", error("Insufficient resources: {0}"))]
    Resource(String),
}

/// An error while instantiating a module.
///
/// This is not a common WebAssembly error, however
/// we need to differentiate from a `LinkError` (an error
/// that happens while linking, on instantiation), a
/// Trap that occurs when calling the WebAssembly module
/// start function, and an error when initializing the user's
/// host environments.
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum InstantiationError {
    /// A linking ocurred during instantiation.
    #[cfg_attr(feature = "std", error(transparent))]
    Link(LinkError),

    /// A runtime error occured while invoking the start function
    #[cfg_attr(feature = "std", error(transparent))]
    Start(RuntimeError),

    /// The module was compiled with a CPU feature that is not available on
    /// the current host.
    #[cfg_attr(feature = "std", error("missing required CPU features: {0:?}"))]
    CpuFeature(String),

    /// Import from a different [`Store`].
    /// This error occurs when an import from a different store is used.
    #[cfg_attr(feature = "std", error("cannot mix imports from different stores"))]
    DifferentStores,

    /// A generic error occured while invoking API functions
    #[cfg_attr(feature = "std", error(transparent))]
    Wasm(WasmError),

    /// Insufficient resources available for execution.
    #[cfg_attr(feature = "std", error("Can't get {0} from the instance exports"))]
    NotInExports(String),
}

impl From<WasmError> for InstantiationError {
    fn from(original: WasmError) -> Self {
        Self::Wasm(original)
    }
}

#[cfg(feature = "core")]
impl std::fmt::Display for InstantiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InstantiationError")
    }
}
