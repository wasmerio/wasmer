use crate::types::{FuncSig, GlobalDesc, Memory, MemoryIndex, Table, TableIndex, Type};

pub type Result<T> = std::result::Result<T, Box<Error>>;
pub type CompileResult<T> = std::result::Result<T, Box<CompileError>>;
pub type LinkResult<T> = std::result::Result<T, Box<LinkError>>;
pub type RuntimeResult<T> = std::result::Result<T, Box<RuntimeError>>;
pub type CallResult<T> = std::result::Result<T, Box<CallError>>;

/// This is returned when the chosen compiler is unable to
/// successfully compile the provided webassembly module into
/// a `Module`.
#[derive(Debug, Clone)]
pub enum CompileError {
    ValidationError { msg: String },
    InternalError { msg: String },
}

/// This is returned when the runtime is unable to
/// correctly link the module with the provided imports.
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
        expected: FuncSig,
        found: FuncSig,
    },
    ImportNotFound {
        namespace: String,
        name: String,
    },
    IncorrectMemoryDescription {
        namespace: String,
        name: String,
        expected: Memory,
        found: Memory,
    },
    IncorrectTableDescription {
        namespace: String,
        name: String,
        expected: Table,
        found: Table,
    },
    IncorrectGlobalDescription {
        namespace: String,
        name: String,
        expected: GlobalDesc,
        found: GlobalDesc,
    },
}

/// This is the error type returned when calling
/// a webassembly function.
///
/// The main way to do this is `Instance.call`.
#[derive(Debug, Clone)]
pub enum RuntimeError {
    OutOfBoundsAccess { memory: MemoryIndex, addr: u32 },
    IndirectCallSignature { table: TableIndex },
    IndirectCallToNull { table: TableIndex },
    Unknown { msg: String },
}

/// This error type is produced by calling a wasm function
/// exported from a module.
///
/// If the module traps in some way while running, this will
/// be the `CallError::Runtime(RuntimeError)` variant.
#[derive(Debug, Clone)]
pub enum CallError {
    Signature { expected: FuncSig, found: Vec<Type> },
    NoSuchExport { name: String },
    ExportNotFunc { name: String },
    Runtime(Box<RuntimeError>),
}

/// The amalgamation of all errors that can occur
/// during the compilation, instantiation, or execution
/// of a webassembly module.
#[derive(Debug, Clone)]
pub enum Error {
    CompileError(Box<CompileError>),
    LinkError(Box<LinkError>),
    RuntimeError(Box<RuntimeError>),
    CallError(Box<CallError>),
}

impl From<Box<CompileError>> for Box<Error> {
    fn from(compile_err: Box<CompileError>) -> Self {
        Box::new(Error::CompileError(compile_err))
    }
}

impl From<Box<LinkError>> for Box<Error> {
    fn from(link_err: Box<LinkError>) -> Self {
        Box::new(Error::LinkError(link_err))
    }
}

impl From<Box<RuntimeError>> for Box<Error> {
    fn from(runtime_err: Box<RuntimeError>) -> Self {
        Box::new(Error::RuntimeError(runtime_err))
    }
}

impl From<Box<CallError>> for Box<Error> {
    fn from(call_err: Box<CallError>) -> Self {
        Box::new(Error::CallError(call_err))
    }
}
