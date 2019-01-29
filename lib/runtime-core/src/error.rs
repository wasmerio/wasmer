use crate::types::{FuncSig, GlobalDesc, MemoryDesc, MemoryIndex, TableDesc, TableIndex, Type};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, Box<Error>>;
pub type CompileResult<T> = std::result::Result<T, Box<CompileError>>;
pub type LinkResult<T> = std::result::Result<T, Vec<LinkError>>;
pub type RuntimeResult<T> = std::result::Result<T, Box<RuntimeError>>;
pub type CallResult<T> = std::result::Result<T, Box<CallError>>;
pub type ResolveResult<T> = std::result::Result<T, Box<ResolveError>>;

/// This is returned when the chosen compiler is unable to
/// successfully compile the provided webassembly module into
/// a `Module`.
///
/// Comparing two `CompileError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum CompileError {
    ValidationError { msg: String },
    InternalError { msg: String },
}

impl PartialEq for CompileError {
    fn eq(&self, _other: &CompileError) -> bool {
        false
    }
}

/// This is returned when the runtime is unable to
/// correctly link the module with the provided imports.
///
/// Comparing two `LinkError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum LinkError {
    IncorrectImportType {
        namespace: String,
        name: String,
        expected: String,
        found: String,
    },
    IncorrectImportSignature {
        namespace: String,
        name: String,
        expected: Arc<FuncSig>,
        found: Arc<FuncSig>,
    },
    ImportNotFound {
        namespace: String,
        name: String,
    },
    IncorrectMemoryDescription {
        namespace: String,
        name: String,
        expected: MemoryDesc,
        found: MemoryDesc,
    },
    IncorrectTableDescription {
        namespace: String,
        name: String,
        expected: TableDesc,
        found: TableDesc,
    },
    IncorrectGlobalDescription {
        namespace: String,
        name: String,
        expected: GlobalDesc,
        found: GlobalDesc,
    },
}

impl PartialEq for LinkError {
    fn eq(&self, _other: &LinkError) -> bool {
        false
    }
}

/// This is the error type returned when calling
/// a webassembly function.
///
/// The main way to do this is `Instance.call`.
///
/// Comparing two `RuntimeError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum RuntimeError {
    OutOfBoundsAccess { memory: MemoryIndex, addr: u32 },
    TableOutOfBounds { table: TableIndex },
    IndirectCallSignature { table: TableIndex },
    IndirectCallToNull { table: TableIndex },
    IllegalArithmeticOperation,
    Unknown { msg: String },
}

impl PartialEq for RuntimeError {
    fn eq(&self, _other: &RuntimeError) -> bool {
        false
    }
}

/// This error type is produced by resolving a wasm function
/// given its name.
///
/// Comparing two `ResolveError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum ResolveError {
    Signature {
        expected: Arc<FuncSig>,
        found: Vec<Type>,
    },
    ExportNotFound {
        name: String,
    },
    ExportWrongType {
        name: String,
    },
}

impl PartialEq for ResolveError {
    fn eq(&self, _other: &ResolveError) -> bool {
        false
    }
}

/// This error type is produced by calling a wasm function
/// exported from a module.
///
/// If the module traps in some way while running, this will
/// be the `CallError::Runtime(RuntimeError)` variant.
///
/// Comparing two `CallError`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum CallError {
    Resolve(ResolveError),
    Runtime(RuntimeError),
}

impl PartialEq for CallError {
    fn eq(&self, _other: &CallError) -> bool {
        false
    }
}

/// The amalgamation of all errors that can occur
/// during the compilation, instantiation, or execution
/// of a webassembly module.
///
/// Comparing two `Error`s always evaluates to false.
#[derive(Debug, Clone)]
pub enum Error {
    CompileError(CompileError),
    LinkError(Vec<LinkError>),
    RuntimeError(RuntimeError),
    ResolveError(ResolveError),
    CallError(CallError),
}

impl PartialEq for Error {
    fn eq(&self, _other: &Error) -> bool {
        false
    }
}

impl From<Box<CompileError>> for Box<Error> {
    fn from(compile_err: Box<CompileError>) -> Self {
        Box::new(Error::CompileError(*compile_err))
    }
}

impl From<Vec<LinkError>> for Box<Error> {
    fn from(link_err: Vec<LinkError>) -> Self {
        Box::new(Error::LinkError(link_err))
    }
}

impl From<Box<RuntimeError>> for Box<Error> {
    fn from(runtime_err: Box<RuntimeError>) -> Self {
        Box::new(Error::RuntimeError(*runtime_err))
    }
}

impl From<Box<ResolveError>> for Box<Error> {
    fn from(resolve_err: Box<ResolveError>) -> Self {
        Box::new(Error::ResolveError(*resolve_err))
    }
}

impl From<Box<CallError>> for Box<Error> {
    fn from(call_err: Box<CallError>) -> Self {
        Box::new(Error::CallError(*call_err))
    }
}

impl From<Box<RuntimeError>> for Box<CallError> {
    fn from(runtime_err: Box<RuntimeError>) -> Self {
        Box::new(CallError::Runtime(*runtime_err))
    }
}

impl From<Box<ResolveError>> for Box<CallError> {
    fn from(resolve_err: Box<ResolveError>) -> Self {
        Box::new(CallError::Resolve(*resolve_err))
    }
}

impl From<CompileError> for Box<Error> {
    fn from(compile_err: CompileError) -> Self {
        Box::new(Error::CompileError(compile_err))
    }
}

impl From<RuntimeError> for Box<Error> {
    fn from(runtime_err: RuntimeError) -> Self {
        Box::new(Error::RuntimeError(runtime_err))
    }
}

impl From<CallError> for Box<Error> {
    fn from(call_err: CallError) -> Self {
        Box::new(Error::CallError(call_err))
    }
}

impl From<RuntimeError> for Box<CallError> {
    fn from(runtime_err: RuntimeError) -> Self {
        Box::new(CallError::Runtime(runtime_err))
    }
}

impl From<ResolveError> for Box<CallError> {
    fn from(resolve_err: ResolveError) -> Self {
        Box::new(CallError::Resolve(resolve_err))
    }
}
