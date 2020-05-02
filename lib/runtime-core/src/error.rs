//! The error module contains the data structures and helper functions used to implement errors that
//! are produced and returned from the wasmer runtime core.
use crate::backend::ExceptionCode;
use crate::types::{FuncSig, GlobalDescriptor, MemoryDescriptor, TableDescriptor, Type};
use core::borrow::Borrow;
use std::any::Any;

/// Aliases the standard `Result` type as `Result` within this module.
pub type Result<T> = std::result::Result<T, Error>;
/// Result of an attempt to compile the provided WebAssembly module into a `Module`.
/// Aliases the standard `Result` with `CompileError` as the default error type.
pub type CompileResult<T> = std::result::Result<T, CompileError>;
/// Result of an attempt to link the provided WebAssembly instance.
/// Aliases the standard `Result` with `Vec<LinkError>` as the default error type.
pub type LinkResult<T> = std::result::Result<T, Vec<LinkError>>;
/// Result of an attempt to run the provided WebAssembly instance.
/// Aliases the standard `Result` with `RuntimeError` as the default error type.
pub type RuntimeResult<T> = std::result::Result<T, RuntimeError>;
/// Result of an attempt to call the provided WebAssembly instance.
/// Aliases the standard `Result` with `CallError` as the default error type.
pub type CallResult<T> = std::result::Result<T, CallError>;
/// Result of an attempt to resolve a WebAssembly function by name.
/// Aliases the standard `Result` with `ResolveError` as the default error type.
pub type ResolveResult<T> = std::result::Result<T, ResolveError>;
/// Result of an attempt to parse bytes into a WebAssembly module.
/// Aliases the standard `Result` with `ParseError` as the default error type.
pub type ParseResult<T> = std::result::Result<T, ParseError>;

/// This is returned when the chosen compiler is unable to
/// successfully compile the provided WebAssembly module into
/// a `Module`.
///
/// Comparing two `CompileError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum CompileError {
    /// A validation error containing an error message.
    ValidationError {
        /// An error message.
        msg: String,
    },
    /// A internal error containing an error message.
    InternalError {
        /// An error message.
        msg: String,
    },
}

impl PartialEq for CompileError {
    fn eq(&self, _other: &CompileError) -> bool {
        false
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CompileError::InternalError { msg } => {
                write!(f, "Internal compiler error: \"{}\"", msg)
            }
            CompileError::ValidationError { msg } => write!(f, "Validation error \"{}\"", msg),
        }
    }
}

impl std::error::Error for CompileError {}

/// This is returned when the runtime is unable to
/// correctly link the module with the provided imports.
///
/// Comparing two `LinkError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum LinkError {
    /// The type of the provided import does not match the expected type.
    IncorrectImportType {
        /// Namespace.
        namespace: String,
        /// Name.
        name: String,
        /// Expected.
        expected: String,
        /// Found.
        found: String,
    },
    /// The signature of the provided import does not match the expected signature.
    IncorrectImportSignature {
        /// Namespace.
        namespace: String,
        /// Name.
        name: String,
        /// Expected.
        expected: FuncSig,
        /// Found.
        found: FuncSig,
    },
    /// An expected import was not provided.
    ImportNotFound {
        /// Namespace.
        namespace: String,
        /// Name.
        name: String,
    },
    /// The memory descriptor provided does not match the expected descriptor.
    IncorrectMemoryDescriptor {
        /// Namespace.
        namespace: String,
        /// Name.
        name: String,
        /// Expected.
        expected: MemoryDescriptor,
        /// Found.
        found: MemoryDescriptor,
    },
    /// The table descriptor provided does not match the expected descriptor.
    IncorrectTableDescriptor {
        /// Namespace.
        namespace: String,
        /// Name.
        name: String,
        /// Expected.
        expected: TableDescriptor,
        /// Found.
        found: TableDescriptor,
    },
    /// The global descriptor provided does not match the expected descriptor.
    IncorrectGlobalDescriptor {
        /// Namespace.
        namespace: String,
        /// Name.
        name: String,
        /// Expected.
        expected: GlobalDescriptor,
        /// Found.
        found: GlobalDescriptor,
    },
    /// A generic error with a message.
    Generic {
        /// Error message.
        message: String,
    },
}

impl PartialEq for LinkError {
    fn eq(&self, _other: &LinkError) -> bool {
        false
    }
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LinkError::ImportNotFound {namespace, name} => write!(f, "Import not found, namespace: {}, name: {}", namespace, name),
            LinkError::IncorrectGlobalDescriptor {namespace, name,expected,found} => {
                write!(f, "Incorrect global descriptor, namespace: {}, name: {}, expected global descriptor: {:?}, found global descriptor: {:?}", namespace, name, expected, found)
            },
            LinkError::IncorrectImportSignature{namespace, name,expected,found} => {
                write!(f, "Incorrect import signature, namespace: {}, name: {}, expected signature: {}, found signature: {}", namespace, name, expected, found)
            }
            LinkError::IncorrectImportType{namespace, name,expected,found} => {
                write!(f, "Incorrect import type, namespace: {}, name: {}, expected type: {}, found type: {}", namespace, name, expected, found)
            }
            LinkError::IncorrectMemoryDescriptor{namespace, name,expected,found} => {
                write!(f, "Incorrect memory descriptor, namespace: {}, name: {}, expected memory descriptor: {:?}, found memory descriptor: {:?}", namespace, name, expected, found)
            },
            LinkError::IncorrectTableDescriptor{namespace, name,expected,found} => {
                write!(f, "Incorrect table descriptor, namespace: {}, name: {}, expected table descriptor: {:?}, found table descriptor: {:?}", namespace, name, expected, found)
            },
            LinkError::Generic { message } => {
                write!(f, "{}", message)
            },
        }
    }
}

impl std::error::Error for LinkError {}

/// An error that happened while invoking a Wasm function.
#[derive(Debug)]
pub enum InvokeError {
    /// Indicates an exceptional circumstance such as a bug in Wasmer (please file an issue!)
    /// or a hardware failure.
    FailedWithNoError,
    /// Indicates that a trap occurred that is not known to Wasmer.
    UnknownTrap {
        /// The address that the trap occurred at.
        address: usize,
        /// The name of the signal.
        signal: &'static str,
    },
    /// A trap that Wasmer knows about occurred.
    TrapCode {
        /// The type of exception.
        code: ExceptionCode,
        /// Where in the Wasm file this trap orginated from.
        srcloc: u32,
    },
    /// A trap occurred that Wasmer knows about but it had a trap code that
    /// we weren't expecting or that we do not handle.  This error may be backend-specific.
    UnknownTrapCode {
        /// The trap code we saw but did not recognize.
        trap_code: String,
        /// Where in the Wasm file this trap orginated from.
        srcloc: u32,
    },
    /// An "early trap" occurred.  TODO: document this properly
    EarlyTrap(Box<RuntimeError>),
    /// Indicates that a breakpoint was hit. The inner value is dependent upon
    /// the middleware or backend being used.
    Breakpoint(Box<RuntimeError>),
}

impl From<InvokeError> for RuntimeError {
    fn from(other: InvokeError) -> RuntimeError {
        match other {
            InvokeError::EarlyTrap(re) | InvokeError::Breakpoint(re) => *re,
            _ => RuntimeError::InvokeError(other),
        }
    }
}

impl std::error::Error for InvokeError {}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            InvokeError::FailedWithNoError => write!(f, "Invoke failed with no error"),
            InvokeError::UnknownTrap { address, signal } => write!(
                f,
                "An unknown trap (`{}`) occured at 0x{:X}",
                signal, address
            ),
            InvokeError::TrapCode { code, srcloc } => {
                write!(f, "A `{}` trap was thrown at code offset {}", code, srcloc)
            }
            InvokeError::UnknownTrapCode { trap_code, srcloc } => write!(
                f,
                "A trap with an unknown trap code (`{}`) was thrown at code offset {}",
                trap_code, srcloc
            ),
            InvokeError::EarlyTrap(rte) => write!(f, "Early trap: {}", rte),
            InvokeError::Breakpoint(rte) => write!(f, "Breakpoint hit: {}", rte),
        }
    }
}

/// A `RuntimeError` is an error that describes why the attempt to fully execute
/// some Wasm has failed.
///
/// These reasons vary from the Wasm trapping or otherwise failing directly to user
/// controlled conditions such as metering running out of gas or a user host function
/// returning a custom error type directly.
#[derive(Debug)]
pub enum RuntimeError {
    /// An error relating to the invocation of a Wasm function.
    InvokeError(InvokeError),
    /// A metering triggered error value.
    ///
    /// An error of this type indicates that it was returned by the metering system.
    Metering(Box<dyn Any + Send>),
    /// A frozen state of Wasm used to pause and resume execution.  Not strictly an
    /// "error", but this happens while executing and therefore is a `RuntimeError`
    /// from the persective of the caller that expected the code to fully execute.
    InstanceImage(Box<dyn Any + Send>),
    /// A user triggered error value.
    ///
    /// An error returned from a host function.
    User(Box<dyn Any + Send>),
}

impl PartialEq for RuntimeError {
    fn eq(&self, _other: &RuntimeError) -> bool {
        false
    }
}

impl std::error::Error for RuntimeError {}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RuntimeError::InvokeError(ie) => write!(f, "Error when calling invoke: {}", ie),
            RuntimeError::Metering(_) => write!(f, "unknown metering error type"),
            RuntimeError::InstanceImage(_) => write!(
                f,
                "Execution interrupted by a suspend signal: instance image returned"
            ),
            RuntimeError::User(user_error) => {
                write!(f, "User supplied error: ")?;
                if let Some(s) = user_error.downcast_ref::<String>() {
                    write!(f, "\"{}\"", s)
                } else if let Some(s) = user_error.downcast_ref::<&str>() {
                    write!(f, "\"{}\"", s)
                } else if let Some(n) = user_error.downcast_ref::<i32>() {
                    write!(f, "{}", n)
                } else {
                    write!(f, "unknown user error type")
                }
            }
        }
    }
}

/// This error type is produced by resolving a wasm function
/// given its name.
///
/// Comparing two `ResolveError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum ResolveError {
    /// Found signature did not match expected signature.
    Signature {
        /// Expected `FuncSig`.
        expected: FuncSig,
        /// Found type.
        found: Vec<Type>,
    },
    /// Export not found.
    ExportNotFound {
        /// Name.
        name: String,
    },
    /// Export found with the wrong type.
    ExportWrongType {
        /// Name.
        name: String,
    },
}

impl PartialEq for ResolveError {
    fn eq(&self, _other: &ResolveError) -> bool {
        false
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ResolveError::ExportNotFound { name } => write!(f, "Export not found: {}", name),
            ResolveError::ExportWrongType { name } => write!(f, "Export wrong type: {}", name),
            ResolveError::Signature { expected, found } => {
                let found = found
                    .as_slice()
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let expected: &FuncSig = expected.borrow();
                write!(
                    f,
                    "Parameters of type [{}] did not match signature {}",
                    found, expected
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// This error type is produced by calling a wasm function
/// exported from a module.
///
/// If the module traps in some way while running, this will
/// be the `CallError::Runtime(RuntimeError)` variant.
///
/// Comparing two `CallError`s always evaluates to false.
pub enum CallError {
    /// An error occured resolving the functions name or types.
    Resolve(ResolveError),
    /// A runtime error occurred during the function call.
    Runtime(RuntimeError),
}

impl PartialEq for CallError {
    fn eq(&self, _other: &CallError) -> bool {
        false
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CallError::Resolve(resolve_error) => write!(f, "Call error: {}", resolve_error),
            CallError::Runtime(runtime_error) => write!(f, "Call error: {}", runtime_error),
        }
    }
}

impl std::fmt::Debug for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CallError::Resolve(resolve_err) => write!(f, "ResolveError: {:?}", resolve_err),
            CallError::Runtime(runtime_err) => write!(f, "RuntimeError: {:?}", runtime_err),
        }
    }
}

impl std::error::Error for CallError {}

/// This error type is produced when creating something,
/// like a `Memory` or a `Table`.
#[derive(Debug, Clone)]
pub enum CreationError {
    /// Unable to create memory error.
    UnableToCreateMemory,
    /// Unable to create table error.
    UnableToCreateTable,
    /// Invalid descriptor error with message.
    InvalidDescriptor(String),
}

impl PartialEq for CreationError {
    fn eq(&self, _other: &CreationError) -> bool {
        false
    }
}

impl std::fmt::Display for CreationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CreationError::UnableToCreateMemory => write!(f, "Unable to Create Memory"),
            CreationError::UnableToCreateTable => write!(f, "Unable to Create Table"),
            CreationError::InvalidDescriptor(msg) => write!(
                f,
                "Unable to create because the supplied descriptor is invalid: \"{}\"",
                msg
            ),
        }
    }
}

impl std::error::Error for CreationError {}

/// The amalgamation of all errors that can occur
/// during the compilation, instantiation, or execution
/// of a WebAssembly module.
///
/// Comparing two `Error`s always evaluates to false.
#[derive(Debug)]
pub enum Error {
    /// Compile error.
    CompileError(CompileError),
    /// Link errors.
    LinkError(Vec<LinkError>),
    /// Runtime error.
    RuntimeError(RuntimeError),
    /// Resolve error.
    ResolveError(ResolveError),
    /// Call error.
    CallError(CallError),
    /// Creation error.
    CreationError(CreationError),
}

impl PartialEq for Error {
    fn eq(&self, _other: &Error) -> bool {
        false
    }
}

impl From<CompileError> for Error {
    fn from(compile_err: CompileError) -> Self {
        Error::CompileError(compile_err)
    }
}

impl From<RuntimeError> for Error {
    fn from(runtime_err: RuntimeError) -> Self {
        Error::RuntimeError(runtime_err)
    }
}

impl From<ResolveError> for Error {
    fn from(resolve_err: ResolveError) -> Self {
        Error::ResolveError(resolve_err)
    }
}

impl From<CallError> for Error {
    fn from(call_err: CallError) -> Self {
        Error::CallError(call_err)
    }
}

impl From<CreationError> for Error {
    fn from(creation_err: CreationError) -> Self {
        Error::CreationError(creation_err)
    }
}

impl From<Vec<LinkError>> for Error {
    fn from(link_errs: Vec<LinkError>) -> Self {
        Error::LinkError(link_errs)
    }
}

impl From<RuntimeError> for CallError {
    fn from(runtime_err: RuntimeError) -> Self {
        CallError::Runtime(runtime_err)
    }
}

impl From<ResolveError> for CallError {
    fn from(resolve_err: ResolveError) -> Self {
        CallError::Resolve(resolve_err)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::CompileError(err) => write!(f, "compile error: {}", err),
            Error::LinkError(errs) => {
                if errs.len() == 1 {
                    write!(f, "link error: {}", errs[0])
                } else {
                    write!(f, "{} link errors:", errs.len())?;
                    for (i, err) in errs.iter().enumerate() {
                        write!(f, " ({} of {}) {}", i + 1, errs.len(), err)?;
                    }
                    Ok(())
                }
            }
            Error::RuntimeError(err) => write!(f, "runtime error: {}", err),
            Error::ResolveError(err) => write!(f, "resolve error: {}", err),
            Error::CallError(err) => write!(f, "call error: {}", err),
            Error::CreationError(err) => write!(f, "creation error: {}", err),
        }
    }
}

impl std::error::Error for Error {}

/// An error occurred while growing a memory or table.
#[derive(Debug)]
pub enum GrowError {
    /// Error growing memory.
    MemoryGrowError,
    /// Error growing table.
    TableGrowError,
    /// Max pages were exceeded.
    ExceededMaxPages(PageError),
    /// Max pages for memory were exceeded.
    ExceededMaxPagesForMemory(usize, usize),
    /// Error protecting memory.
    CouldNotProtectMemory(MemoryProtectionError),
    /// Error creating memory.
    CouldNotCreateMemory(MemoryCreationError),
}

impl std::fmt::Display for GrowError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            GrowError::MemoryGrowError => write!(f, "Unable to grow memory"),
            GrowError::TableGrowError => write!(f, "Unable to grow table"),
            GrowError::ExceededMaxPages(e) => write!(f, "Grow Error: {}", e),
            GrowError::ExceededMaxPagesForMemory(left, added) => write!(f, "Failed to add pages because would exceed maximum number of pages for the memory. Left: {}, Added: {}", left, added),
            GrowError::CouldNotCreateMemory(e) => write!(f, "Grow Error: {}", e),
            GrowError::CouldNotProtectMemory(e) => write!(f, "Grow Error: {}", e),
        }
    }
}

impl std::error::Error for GrowError {}

/// A kind of page error.
#[derive(Debug)]
pub enum PageError {
    // left, right, added
    /// Max pages were exceeded error.
    ExceededMaxPages(usize, usize, usize),
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PageError::ExceededMaxPages(left, right, added) => write!(f, "Failed to add pages because would exceed maximum number of pages. Left: {}, Right: {}, Pages added: {}", left, right, added),
        }
    }
}
impl std::error::Error for PageError {}

impl Into<GrowError> for PageError {
    fn into(self) -> GrowError {
        GrowError::ExceededMaxPages(self)
    }
}

/// Error occured while creating memory.
#[derive(Debug)]
pub enum MemoryCreationError {
    /// Allocation of virtual memory failed error.
    VirtualMemoryAllocationFailed(usize, String),
    /// Error creating memory from file.
    CouldNotCreateMemoryFromFile(std::io::Error),
}

impl std::fmt::Display for MemoryCreationError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            MemoryCreationError::VirtualMemoryAllocationFailed(size, msg) => write!(
                f,
                "Allocation virtual memory with size {} failed. \nErrno message: {}",
                size, msg
            ),
            MemoryCreationError::CouldNotCreateMemoryFromFile(e) => write!(f, "IO Error: {}", e),
        }
    }
}
impl std::error::Error for MemoryCreationError {}

impl Into<GrowError> for MemoryCreationError {
    fn into(self) -> GrowError {
        GrowError::CouldNotCreateMemory(self)
    }
}

impl From<std::io::Error> for MemoryCreationError {
    fn from(io_error: std::io::Error) -> Self {
        MemoryCreationError::CouldNotCreateMemoryFromFile(io_error)
    }
}

/// Error protecting memory.
#[derive(Debug)]
pub enum MemoryProtectionError {
    /// Protection failed error.
    ProtectionFailed(usize, usize, String),
}

impl std::fmt::Display for MemoryProtectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            MemoryProtectionError::ProtectionFailed(start, size, msg) => write!(
                f,
                "Allocation virtual memory starting at {} with size {} failed. \nErrno message: {}",
                start, size, msg
            ),
        }
    }
}
impl std::error::Error for MemoryProtectionError {}

impl Into<GrowError> for MemoryProtectionError {
    fn into(self) -> GrowError {
        GrowError::CouldNotProtectMemory(self)
    }
}

/// Parse Error.
#[derive(Debug)]
pub enum ParseError {
    /// Error reading binary.
    BinaryReadError,
}

impl From<wasmparser::BinaryReaderError> for ParseError {
    fn from(_: wasmparser::BinaryReaderError) -> Self {
        ParseError::BinaryReadError
    }
}
