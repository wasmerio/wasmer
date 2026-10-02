use std::path::PathBuf;

use crate::SpawnError;
use virtual_fs::FsError;
use wasmer::{ExportError, ExternType, InstantiationError, MemoryError, RuntimeError};

use super::ModuleHandle;

#[derive(thiserror::Error, Debug)]
pub enum LinkError {
    #[error("Dynamic-link synchronization aborted with exit code {0}")]
    SynchronizationAborted(wasmer_wasix_types::wasi::ExitCode),

    #[error("Cannot access linker through a dead instance group")]
    InstanceGroupIsDead,

    #[error("Main module is missing a required import: {0}")]
    MissingMainModuleImport(String),

    #[error("Failed to spawn module: {0}")]
    SpawnError(#[from] SpawnError),

    #[error("Failed to instantiate module: {0}")]
    InstantiationError(#[from] InstantiationError),

    #[error("Memory allocation error: {0}")]
    MemoryAllocationError(#[from] MemoryError),

    #[error("Failed to allocate function table indices: {0}")]
    TableAllocationError(RuntimeError),

    #[error("Failed to find shared library {0}: {1}")]
    SharedLibraryMissing(String, LocateModuleError),

    #[error("Module is not a dynamic library")]
    NotDynamicLibrary,

    #[error("Module's memory is not shared")]
    MemoryNotShared,

    #[error("Failed to parse dylink.0 section: {0}")]
    Dylink0SectionParseError(#[from] wasmparser::BinaryReaderError),

    #[error("Unresolved global '{0}'.{1} due to: {2}")]
    UnresolvedGlobal(String, String, Box<ResolveError>),

    #[error("Failed to update global {0} due to: {1}")]
    GlobalUpdateFailed(String, RuntimeError),

    #[error("Expected global to be of type I32 or I64: '{0}'.{1}")]
    NonIntegerGlobal(String, String),

    #[error("Bad known import: '{0}'.{1} of type {2:?}")]
    BadImport(String, String, ExternType),

    #[error(
        "Import could not be satisfied because of type mismatch: '{0}'.{1}, expected {2:?}, found {3:?}"
    )]
    ImportTypeMismatch(String, String, ExternType, ExternType),

    #[error("Expected import to be a function: '{0}'.{1}")]
    ImportMustBeFunction(&'static str, String),

    #[error("Expected export {0} to be a function, found: {1:?}")]
    ExportMustBeFunction(String, ExternType),

    #[error("Failed to initialize instance: {0}")]
    InitializationError(anyhow::Error),

    #[error("Runtime hook failed: {0}")]
    RuntimeHookError(anyhow::Error),

    #[error("Initialization function has invalid signature: {0}")]
    InitFuncWithInvalidSignature(String),

    #[error("Initialization function {0} failed to run: {1}")]
    InitFunctionFailed(String, RuntimeError),

    #[error("Failed to initialize WASI(X) module handles: {0}")]
    MainModuleHandleInitFailed(ExportError),

    #[error("Bad __tls_base export, expected a global of type I32 or I64")]
    BadTlsBaseExport,

    #[error(
        "TLS symbol {0} cannot be resolved from module {1} because it does not export its __tls_base"
    )]
    MissingTlsBaseExport(String, ModuleHandle),
}

impl LinkError {
    pub(crate) fn termination_code(&self) -> Option<wasmer_wasix_types::wasi::ExitCode> {
        match self {
            Self::SynchronizationAborted(code) => Some(*code),
            Self::SpawnError(SpawnError::Runtime(error)) => error.as_exit_code(),
            Self::InitFunctionFailed(_, error)
            | Self::GlobalUpdateFailed(_, error)
            | Self::TableAllocationError(error)
            | Self::InstantiationError(InstantiationError::Start(error)) => {
                runtime_exit_code(error)
            }
            Self::InitializationError(error) | Self::RuntimeHookError(error) => {
                if let Some(crate::WasiError::Exit(code)) = error.downcast_ref::<crate::WasiError>()
                {
                    Some(*code)
                } else {
                    error
                        .downcast_ref::<RuntimeError>()
                        .and_then(runtime_exit_code)
                }
            }
            Self::UnresolvedGlobal(_, _, error) => error.termination_code(),
            _ => None,
        }
    }
}

fn runtime_exit_code(error: &RuntimeError) -> Option<wasmer_wasix_types::wasi::ExitCode> {
    match error.downcast_ref::<crate::WasiError>() {
        Some(crate::WasiError::Exit(code)) => Some(*code),
        _ => None,
    }
}

#[derive(Debug)]
pub enum LocateModuleError {
    Single(FsError),
    Multiple(Vec<(PathBuf, FsError)>),
}

impl std::fmt::Display for LocateModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocateModuleError::Single(e) => std::fmt::Display::fmt(&e, f),
            LocateModuleError::Multiple(errors) => {
                for (path, error) in errors {
                    write!(f, "\n    {}: {}", path.display(), error)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ResolveError {
    #[error("Linker not initialized")]
    NotInitialized,

    #[error("Invalid module handle")]
    InvalidModuleHandle,

    #[error("Missing export")]
    MissingExport,

    #[error("Invalid export type: {0:?}")]
    InvalidExportType(ExternType),

    #[error("Failed to allocate function table indices: {0}")]
    TableAllocationError(RuntimeError),

    #[error("Cannot access linker through a dead instance group")]
    InstanceGroupIsDead,

    #[error("Failed to perform pending DL operation: {0}")]
    PendingDlOperationFailed(#[from] LinkError),

    #[error("Module must export its __tls_base for exported TLS symbols to be resolved correctly")]
    NoTlsBaseGlobalExport,
}

impl ResolveError {
    pub(crate) fn termination_code(&self) -> Option<wasmer_wasix_types::wasi::ExitCode> {
        match self {
            Self::PendingDlOperationFailed(error) => error.termination_code(),
            Self::TableAllocationError(error) => runtime_exit_code(error),
            _ => None,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn wrapped_guest_exit_remains_fatal_without_published_process_status() {
        let runtime_error = || RuntimeError::user(Box::new(crate::WasiError::Exit(137.into())));
        for error in [
            LinkError::InitFunctionFailed("init".into(), runtime_error()),
            LinkError::InstantiationError(InstantiationError::Start(runtime_error())),
            LinkError::RuntimeHookError(anyhow::Error::new(runtime_error()).context("hook failed")),
            LinkError::SpawnError(SpawnError::Runtime(crate::WasiRuntimeError::Wasi(
                crate::WasiError::Exit(137.into()),
            ))),
            LinkError::SpawnError(SpawnError::Runtime(crate::WasiRuntimeError::Runtime(
                runtime_error(),
            ))),
        ] {
            assert_eq!(error.termination_code(), Some(137.into()));
            assert_eq!(
                ResolveError::PendingDlOperationFailed(error).termination_code(),
                Some(137.into())
            );
        }
        assert_eq!(
            LinkError::InitFunctionFailed("init".into(), RuntimeError::new("ordinary error"))
                .termination_code(),
            None
        );
    }
}
