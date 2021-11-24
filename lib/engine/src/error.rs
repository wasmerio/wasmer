//! The WebAssembly possible errors
use crate::trap::RuntimeError;
use std::io;
use thiserror::Error;
use wasmer_compiler::CompileError;
use wasmer_types::ExternType;

/// The Serialize error can occur when serializing a
/// compiled Module into a binary.
#[derive(Error, Debug)]
pub enum SerializeError {
    /// An IO error
    #[error(transparent)]
    Io(#[from] io::Error),
    /// A generic serialization error
    #[error("{0}")]
    Generic(String),
}

/// The Deserialize error can occur when loading a
/// compiled Module from a binary.
#[derive(Error, Debug)]
pub enum DeserializeError {
    /// An IO error
    #[error(transparent)]
    Io(#[from] io::Error),
    /// A generic deserialization error
    #[error("{0}")]
    Generic(String),
    /// Incompatible serialized binary
    #[error("incompatible binary: {0}")]
    Incompatible(String),
    /// The provided binary is corrupted
    #[error("corrupted binary: {0}")]
    CorruptedBinary(String),
    /// The binary was valid, but we got an error when
    /// trying to allocate the required resources.
    #[error(transparent)]
    Compiler(CompileError),
}

/// An ImportError.
///
/// Note: this error is not standard to WebAssembly, but it's
/// useful to determine the import issue on the API side.
#[derive(Error, Debug)]
pub enum ImportError {
    /// Incompatible Import Type.
    /// This error occurs when the import types mismatch.
    #[error("incompatible import type. Expected {0:?} but received {1:?}")]
    IncompatibleType(ExternType, ExternType),

    /// Unknown Import.
    /// This error occurs when an import was expected but not provided.
    #[error("unknown import. Expected {0:?}")]
    UnknownImport(ExternType),
}

/// The WebAssembly.LinkError object indicates an error during
/// module instantiation (besides traps from the start function).
///
/// This is based on the [link error][link-error] API.
///
/// [link-error]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/WebAssembly/LinkError
#[derive(Error, Debug)]
#[error("Link error: {0}")]
pub enum LinkError {
    /// An error occurred when checking the import types.
    #[error("Error while importing {0:?}.{1:?}: {2}")]
    Import(String, String, ImportError),

    /// A trap ocurred during linking.
    #[error("RuntimeError occurred during linking: {0}")]
    Trap(#[source] RuntimeError),

    /// Insufficient resources available for linking.
    #[error("Insufficient resources: {0}")]
    Resource(String),
}

/// An error while instantiating a module.
///
/// This is not a common WebAssembly error, however
/// we need to differentiate from a `LinkError` (an error
/// that happens while linking, on instantiation) and a
/// Trap that occurs when calling the WebAssembly module
/// start function.
#[derive(Error, Debug)]
pub enum InstantiationError {
    /// A linking ocurred during instantiation.
    #[error(transparent)]
    Link(LinkError),

    /// The module was compiled with a CPU feature that is not available on
    /// the current host.
    #[error("module compiled with CPU feature that is missing from host")]
    CpuFeature(String),

    /// A runtime error occured while invoking the start function
    #[error(transparent)]
    Start(RuntimeError),
}
