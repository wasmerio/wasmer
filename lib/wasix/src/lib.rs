// FIXME: merge with ./lib.rs_upstream

#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

//! Wasmer's WASI implementation
//!
//! Use `generate_import_object` to create an [`Imports`].  This [`Imports`]
//! can be combined with a module to create an `Instance` which can execute WASI
//! Wasm functions.
//!
//! See `state` for the experimental WASI FS API.  Also see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/main/examples/plugin.rs)
//! for an example of how to extend WASI using the WASI FS API.

#[cfg(all(not(feature = "sys"), not(feature = "js")))]
compile_error!("At least the `sys` or the `js` feature must be enabled. Please, pick one.");

#[cfg(all(feature = "sys", feature = "js"))]
compile_error!(
    "Cannot have both `sys` and `js` features enabled at the same time. Please, pick one."
);

#[cfg(all(feature = "sys", target_arch = "wasm32"))]
compile_error!("The `sys` feature must be enabled only for non-`wasm32` target.");

#[cfg(all(feature = "js", not(target_arch = "wasm32")))]
compile_error!(
    "The `js` feature must be enabled only for the `wasm32` target (either `wasm32-unknown-unknown` or `wasm32-wasip1`)."
);

#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(test)]
#[macro_use]
extern crate pretty_assertions;

#[macro_use]
mod macros;
pub mod bin_factory;
pub mod os;
// TODO: should this be pub?
pub mod net;
// TODO: should this be pub?
pub mod capabilities;
pub mod fs;
pub mod http;
pub mod journal;
mod rewind;
pub mod runners;
pub mod runtime;
mod state;
mod syscalls;
mod utils;

use std::sync::Arc;

#[allow(unused_imports)]
use bytes::{Bytes, BytesMut};
use os::task::control_plane::ControlPlaneError;
use thiserror::Error;
use tracing::error;
// re-exports needed for OS
pub use wasmer;
pub use wasmer_wasix_types;

use wasmer::{
    imports, namespace, AsStoreMut, Exports, FunctionEnv, Imports, Memory32, MemoryAccessError,
    MemorySize, RuntimeError,
};

pub use virtual_fs;
pub use virtual_fs::{DuplexPipe, FsError, Pipe, VirtualFile, WasiBidirectionalSharedPipePair};
pub use virtual_net;
pub use virtual_net::{UnsupportedVirtualNetworking, VirtualNetworking};

#[cfg(feature = "host-vnet")]
pub use virtual_net::{
    host::{LocalNetworking, LocalTcpListener, LocalTcpStream, LocalUdpSocket},
    io_err_into_net_error,
};
use wasmer_wasix_types::wasi::{Errno, ExitCode};

pub use crate::{
    fs::{default_fs_backing, Fd, WasiFs, WasiInodes, VIRTUAL_ROOT_FD},
    os::{
        task::{
            control_plane::WasiControlPlane,
            process::{WasiProcess, WasiProcessId},
            thread::{WasiThread, WasiThreadError, WasiThreadHandle, WasiThreadId},
        },
        WasiTtyState,
    },
    rewind::*,
    runtime::{task_manager::VirtualTaskManager, PluggableRuntime, Runtime},
    state::{
        WasiEnv, WasiEnvBuilder, WasiEnvInit, WasiFunctionEnv, WasiInstanceHandles,
        WasiStateCreationError, ALL_RIGHTS,
    },
    syscalls::{journal::wait_for_snapshot, rewind, rewind_ext, types, unwind},
    utils::is_wasix_module,
    utils::{
        get_wasi_version, get_wasi_versions, is_wasi_module,
        store::{capture_store_snapshot, restore_store_snapshot, StoreSnapshot},
        WasiVersion,
    },
};

/// This is returned in `RuntimeError`.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
#[derive(Error, Debug)]
pub enum WasiError {
    #[error("WASI exited with code: {0}")]
    Exit(ExitCode),
    #[error("WASI thread exited")]
    ThreadExit,
    #[error("WASI deep sleep: {0:?}")]
    DeepSleep(DeepSleepWork),
    #[error("The WASI version could not be determined")]
    UnknownWasiVersion,
}

pub type WasiResult<T> = Result<Result<T, Errno>, WasiError>;

#[deny(unused, dead_code)]
#[derive(Error, Debug)]
pub enum SpawnError {
    /// Failed during serialization
    #[error("serialization failed")]
    Serialization,
    /// Failed during deserialization
    #[error("deserialization failed")]
    Deserialization,
    /// Invalid Wasmer process
    #[error("invalid wasmer")]
    InvalidWasmer,
    /// Failed to fetch the Wasmer process
    #[error("fetch failed")]
    FetchFailed,
    #[error(transparent)]
    CacheError(crate::runtime::module_cache::CacheError),
    /// Failed to compile the Wasmer process
    #[error("compile error: {error:?}")]
    CompileError {
        module_hash: wasmer_types::ModuleHash,
        error: wasmer::CompileError,
    },
    /// Invalid ABI
    #[error("Wasmer process has an invalid ABI")]
    InvalidABI,
    /// Bad handle
    #[error("bad handle")]
    BadHandle,
    /// Call is unsupported
    #[error("unsupported")]
    Unsupported,
    /// Not found
    #[error("not found: {message}")]
    NotFound { message: String },
    /// Tried to run the specified binary as a new WASI thread/process, but
    /// the binary name was not found.
    #[error("could not find binary '{binary}'")]
    BinaryNotFound { binary: String },
    #[error("could not find an entrypoint in the package '{package_id}'")]
    MissingEntrypoint {
        package_id: wasmer_config::package::PackageId,
    },
    #[error("could not load ")]
    ModuleLoad { message: String },
    /// Bad request
    #[error("bad request")]
    BadRequest,
    /// Access denied
    #[error("access denied")]
    AccessDenied,
    /// Internal error has occurred
    #[error("internal error")]
    InternalError,
    /// An error occurred while preparing the file system
    #[error(transparent)]
    FileSystemError(ExtendedFsError),
    /// Memory allocation failed
    #[error("memory allocation failed")]
    MemoryAllocationFailed,
    /// Memory access violation
    #[error("memory access violation")]
    MemoryAccessViolation,
    /// Some other unhandled error. If you see this, it's probably a bug.
    #[error("unknown error found")]
    UnknownError,
    #[error("runtime error")]
    Runtime(#[from] WasiRuntimeError),
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug)]
pub struct ExtendedFsError {
    pub error: virtual_fs::FsError,
    pub message: Option<String>,
}

impl ExtendedFsError {
    pub fn with_msg(error: virtual_fs::FsError, msg: impl Into<String>) -> Self {
        Self {
            error,
            message: Some(msg.into()),
        }
    }

    pub fn new(error: virtual_fs::FsError) -> Self {
        Self {
            error,
            message: None,
        }
    }
}

impl std::fmt::Display for ExtendedFsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "fs error: {}", self.error)?;

        if let Some(msg) = &self.message {
            write!(f, " | {msg}")?;
        }

        Ok(())
    }
}

impl std::error::Error for ExtendedFsError {
    fn cause(&self) -> Option<&dyn std::error::Error> {
        Some(&self.error)
    }
}

impl SpawnError {
    /// Returns `true` if the spawn error is [`NotFound`].
    ///
    /// [`NotFound`]: SpawnError::NotFound
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::NotFound { .. } | Self::MissingEntrypoint { .. } | Self::BinaryNotFound { .. }
        )
    }
}

#[derive(thiserror::Error, Debug)]
pub enum WasiRuntimeError {
    #[error("WASI state setup failed")]
    Init(#[from] WasiStateCreationError),
    #[error("Loading exports failed")]
    Export(#[from] wasmer::ExportError),
    #[error("Instantiation failed")]
    Instantiation(#[from] wasmer::InstantiationError),
    #[error("WASI error")]
    Wasi(#[from] WasiError),
    #[error("Process manager error")]
    ControlPlane(#[from] ControlPlaneError),
    #[error("{0}")]
    Runtime(#[from] RuntimeError),
    #[error("Memory access error")]
    Thread(#[from] WasiThreadError),
    #[error("{0}")]
    Anyhow(#[from] Arc<anyhow::Error>),
}

impl WasiRuntimeError {
    /// Retrieve the concrete exit code returned by an instance.
    ///
    /// Returns [`None`] if a general execution error ocurred.
    pub fn as_exit_code(&self) -> Option<ExitCode> {
        if let WasiRuntimeError::Wasi(WasiError::Exit(code)) = self {
            Some(*code)
        } else if let WasiRuntimeError::Runtime(err) = self {
            if let Some(WasiError::Exit(code)) = err.downcast_ref() {
                Some(*code)
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn run_wasi_func(
    func: &wasmer::Function,
    store: &mut impl AsStoreMut,
    params: &[wasmer::Value],
) -> Result<Box<[wasmer::Value]>, WasiRuntimeError> {
    func.call(store, params).map_err(|err| {
        if let Some(_werr) = err.downcast_ref::<WasiError>() {
            let werr = err.downcast::<WasiError>().unwrap();
            WasiRuntimeError::Wasi(werr)
        } else {
            WasiRuntimeError::Runtime(err)
        }
    })
}

/// Run a main function.
///
/// This is usually called "_start" in WASI modules.
/// The function will not receive arguments or return values.
///
/// An exit code that is not 0 will be returned as a `WasiError::Exit`.
#[allow(clippy::result_large_err)]
pub(crate) fn run_wasi_func_start(
    func: &wasmer::Function,
    store: &mut impl AsStoreMut,
) -> Result<(), WasiRuntimeError> {
    run_wasi_func(func, store, &[])?;
    Ok(())
}

#[derive(Debug)]
pub struct WasiVFork {
    /// The unwound stack before the vfork occured
    pub rewind_stack: BytesMut,
    /// The mutable parts of the store
    pub store_data: Bytes,
    /// The environment before the vfork occured
    pub env: Box<WasiEnv>,

    /// Handle of the thread we have forked (dropping this handle
    /// will signal that the thread is dead)
    pub handle: WasiThreadHandle,

    is_64bit: bool,
}

impl Clone for WasiVFork {
    fn clone(&self) -> Self {
        Self {
            rewind_stack: self.rewind_stack.clone(),
            store_data: self.store_data.clone(),
            env: Box::new(self.env.as_ref().clone()),
            handle: self.handle.clone(),
            is_64bit: self.is_64bit,
        }
    }
}

/// Create an [`Imports`] with an existing [`WasiEnv`]. `WasiEnv`
/// needs a [`WasiState`], that can be constructed from a
/// [`WasiEnvBuilder`](state::WasiEnvBuilder).
pub fn generate_import_object_from_env(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
    version: WasiVersion,
) -> Imports {
    let mut imports = match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(store, ctx),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(store, ctx)
        }
        WasiVersion::Wasix32v1 => generate_import_object_wasix32_v1(store, ctx),
        WasiVersion::Wasix64v1 => generate_import_object_wasix64_v1(store, ctx),
    };

    let exports_wasi_generic = wasi_exports_generic(store, ctx);

    let imports_wasi_generic = imports! {
        "wasi" => exports_wasi_generic,
    };

    imports.extend(&imports_wasi_generic);

    imports
}

fn wasi_exports_generic(mut store: &mut impl AsStoreMut, env: &FunctionEnv<WasiEnv>) -> Exports {
    use syscalls::*;
    let namespace = namespace! {
        "thread-spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn::<Memory32>),
    };
    namespace
}

fn wasi_unstable_exports(mut store: &mut impl AsStoreMut, env: &FunctionEnv<WasiEnv>) -> Exports {
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory32>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory32>),
        "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get::<Memory32>),
        "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
        "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
        "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
        "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
        "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::fd_filestat_get),
        "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
        "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read::<Memory32>),
        "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::fd_seek),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory32>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory32>),
        "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::path_filestat_get),
        "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_typed_with_env(&mut store, env, path_link::<Memory32>),
        "path_open" => Function::new_typed_with_env(&mut store, env, path_open::<Memory32>),
        "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename::<Memory32>),
        "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::poll_oneoff::<Memory32>),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory32>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield::<Memory32>),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
        "thread-spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn::<Memory32>),
    };
    namespace
}

fn wasi_snapshot_preview1_exports(
    mut store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Exports {
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory32>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory32>),
        "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get::<Memory32>),
        "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
        "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
        "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
        "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
        "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, fd_filestat_get::<Memory32>),
        "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
        "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read::<Memory32>),
        "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek::<Memory32>),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory32>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory32>),
        "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_typed_with_env(&mut store, env, path_filestat_get::<Memory32>),
        "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_typed_with_env(&mut store, env, path_link::<Memory32>),
        "path_open" => Function::new_typed_with_env(&mut store, env, path_open::<Memory32>),
        "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename::<Memory32>),
        "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, poll_oneoff::<Memory32>),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory32>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield::<Memory32>),
        "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept::<Memory32>),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
        "thread-spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn::<Memory32>),
    };
    namespace
}

fn wasix_exports_32(mut store: &mut impl AsStoreMut, env: &FunctionEnv<WasiEnv>) -> Exports {
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory32>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory32>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory32>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory32>),
        "clock_time_set" => Function::new_typed_with_env(&mut store, env, clock_time_set::<Memory32>),
        "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get::<Memory32>),
        "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get::<Memory32>),
        "epoll_create" => Function::new_typed_with_env(&mut store, env, epoll_create::<Memory32>),
        "epoll_ctl" => Function::new_typed_with_env(&mut store, env, epoll_ctl::<Memory32>),
        "epoll_wait" => Function::new_typed_with_env(&mut store, env, epoll_wait::<Memory32>),
        "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
        "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
        "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
        "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
        "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get::<Memory32>),
        "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, fd_filestat_get::<Memory32>),
        "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
        "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread::<Memory32>),
        "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get::<Memory32>),
        "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name::<Memory32>),
        "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite::<Memory32>),
        "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read::<Memory32>),
        "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir::<Memory32>),
        "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
        "fd_dup" => Function::new_typed_with_env(&mut store, env, fd_dup::<Memory32>),
        "fd_dup2" => Function::new_typed_with_env(&mut store, env, fd_dup2::<Memory32>),
        "fd_fdflags_get" => Function::new_typed_with_env(&mut store, env, fd_fdflags_get::<Memory32>),
        "fd_fdflags_set" => Function::new_typed_with_env(&mut store, env, fd_fdflags_set),
        "fd_event" => Function::new_typed_with_env(&mut store, env, fd_event::<Memory32>),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek::<Memory32>),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory32>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory32>),
        "fd_pipe" => Function::new_typed_with_env(&mut store, env, fd_pipe::<Memory32>),
        "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory::<Memory32>),
        "path_filestat_get" => Function::new_typed_with_env(&mut store, env, path_filestat_get::<Memory32>),
        "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times::<Memory32>),
        "path_link" => Function::new_typed_with_env(&mut store, env, path_link::<Memory32>),
        "path_open" => Function::new_typed_with_env(&mut store, env, path_open::<Memory32>),
        "path_open2" => Function::new_typed_with_env(&mut store, env, path_open2::<Memory32>),
        "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink::<Memory32>),
        "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory::<Memory32>),
        "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename::<Memory32>),
        "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink::<Memory32>),
        "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file::<Memory32>),
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, poll_oneoff::<Memory32>),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory32>),
        "proc_fork" => Function::new_typed_with_env(&mut store, env, proc_fork::<Memory32>),
        "proc_join" => Function::new_typed_with_env(&mut store, env, proc_join::<Memory32>),
        "proc_signal" => Function::new_typed_with_env(&mut store, env, proc_signal::<Memory32>),
        "proc_exec" => Function::new_typed_with_env(&mut store, env, proc_exec::<Memory32>),
        "proc_exec2" => Function::new_typed_with_env(&mut store, env, proc_exec2::<Memory32>),
        "proc_exec3" => Function::new_typed_with_env(&mut store, env, proc_exec3::<Memory32>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "proc_raise_interval" => Function::new_typed_with_env(&mut store, env, proc_raise_interval),
        "proc_snapshot" => Function::new_typed_with_env(&mut store, env, proc_snapshot::<Memory32>),
        "proc_spawn" => Function::new_typed_with_env(&mut store, env, proc_spawn::<Memory32>),
        "proc_id" => Function::new_typed_with_env(&mut store, env, proc_id::<Memory32>),
        "proc_parent" => Function::new_typed_with_env(&mut store, env, proc_parent::<Memory32>),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get::<Memory32>),
        "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set::<Memory32>),
        "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd::<Memory32>),
        "chdir" => Function::new_typed_with_env(&mut store, env, chdir::<Memory32>),
        "callback_signal" => Function::new_typed_with_env(&mut store, env, callback_signal::<Memory32>),
        "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn_v2::<Memory32>),
        "thread_spawn_v2" => Function::new_typed_with_env(&mut store, env, thread_spawn_v2::<Memory32>),
        "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep::<Memory32>),
        "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id::<Memory32>),
        "thread_signal" => Function::new_typed_with_env(&mut store, env, thread_signal),
        "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join::<Memory32>),
        "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism::<Memory32>),
        "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield::<Memory32>),
        "stack_checkpoint" => Function::new_typed_with_env(&mut store, env, stack_checkpoint::<Memory32>),
        "stack_restore" => Function::new_typed_with_env(&mut store, env, stack_restore::<Memory32>),
        "futex_wait" => Function::new_typed_with_env(&mut store, env, futex_wait::<Memory32>),
        "futex_wake" => Function::new_typed_with_env(&mut store, env, futex_wake::<Memory32>),
        "futex_wake_all" => Function::new_typed_with_env(&mut store, env, futex_wake_all::<Memory32>),
        "port_bridge" => Function::new_typed_with_env(&mut store, env, port_bridge::<Memory32>),
        "port_unbridge" => Function::new_typed_with_env(&mut store, env, port_unbridge),
        "port_dhcp_acquire" => Function::new_typed_with_env(&mut store, env, port_dhcp_acquire),
        "port_addr_add" => Function::new_typed_with_env(&mut store, env, port_addr_add::<Memory32>),
        "port_addr_remove" => Function::new_typed_with_env(&mut store, env, port_addr_remove::<Memory32>),
        "port_addr_clear" => Function::new_typed_with_env(&mut store, env, port_addr_clear),
        "port_addr_list" => Function::new_typed_with_env(&mut store, env, port_addr_list::<Memory32>),
        "port_mac" => Function::new_typed_with_env(&mut store, env, port_mac::<Memory32>),
        "port_gateway_set" => Function::new_typed_with_env(&mut store, env, port_gateway_set::<Memory32>),
        "port_route_add" => Function::new_typed_with_env(&mut store, env, port_route_add::<Memory32>),
        "port_route_remove" => Function::new_typed_with_env(&mut store, env, port_route_remove::<Memory32>),
        "port_route_clear" => Function::new_typed_with_env(&mut store, env, port_route_clear),
        "port_route_list" => Function::new_typed_with_env(&mut store, env, port_route_list::<Memory32>),
        "sock_status" => Function::new_typed_with_env(&mut store, env, sock_status::<Memory32>),
        "sock_addr_local" => Function::new_typed_with_env(&mut store, env, sock_addr_local::<Memory32>),
        "sock_addr_peer" => Function::new_typed_with_env(&mut store, env, sock_addr_peer::<Memory32>),
        "sock_open" => Function::new_typed_with_env(&mut store, env, sock_open::<Memory32>),
        "sock_pair" => Function::new_typed_with_env(&mut store, env, sock_pair::<Memory32>),
        "sock_set_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_set_opt_flag),
        "sock_get_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_get_opt_flag::<Memory32>),
        "sock_set_opt_time" => Function::new_typed_with_env(&mut store, env, sock_set_opt_time::<Memory32>),
        "sock_get_opt_time" => Function::new_typed_with_env(&mut store, env, sock_get_opt_time::<Memory32>),
        "sock_set_opt_size" => Function::new_typed_with_env(&mut store, env, sock_set_opt_size),
        "sock_get_opt_size" => Function::new_typed_with_env(&mut store, env, sock_get_opt_size::<Memory32>),
        "sock_join_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v4::<Memory32>),
        "sock_leave_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v4::<Memory32>),
        "sock_join_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v6::<Memory32>),
        "sock_leave_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v6::<Memory32>),
        "sock_bind" => Function::new_typed_with_env(&mut store, env, sock_bind::<Memory32>),
        "sock_listen" => Function::new_typed_with_env(&mut store, env, sock_listen::<Memory32>),
        "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept_v2::<Memory32>),
        "sock_accept_v2" => Function::new_typed_with_env(&mut store, env, sock_accept_v2::<Memory32>),
        "sock_connect" => Function::new_typed_with_env(&mut store, env, sock_connect::<Memory32>),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_recv_from" => Function::new_typed_with_env(&mut store, env, sock_recv_from::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_send_to" => Function::new_typed_with_env(&mut store, env, sock_send_to::<Memory32>),
        "sock_send_file" => Function::new_typed_with_env(&mut store, env, sock_send_file::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
        "resolve" => Function::new_typed_with_env(&mut store, env, resolve::<Memory32>),
    };
    namespace
}

fn wasix_exports_64(mut store: &mut impl AsStoreMut, env: &FunctionEnv<WasiEnv>) -> Exports {
    use syscalls::*;
    let namespace = namespace! {
        "args_get" => Function::new_typed_with_env(&mut store, env, args_get::<Memory64>),
        "args_sizes_get" => Function::new_typed_with_env(&mut store, env, args_sizes_get::<Memory64>),
        "clock_res_get" => Function::new_typed_with_env(&mut store, env, clock_res_get::<Memory64>),
        "clock_time_get" => Function::new_typed_with_env(&mut store, env, clock_time_get::<Memory64>),
        "clock_time_set" => Function::new_typed_with_env(&mut store, env, clock_time_set::<Memory64>),
        "environ_get" => Function::new_typed_with_env(&mut store, env, environ_get::<Memory64>),
        "environ_sizes_get" => Function::new_typed_with_env(&mut store, env, environ_sizes_get::<Memory64>),
        "epoll_create" => Function::new_typed_with_env(&mut store, env, epoll_create::<Memory64>),
        "epoll_ctl" => Function::new_typed_with_env(&mut store, env, epoll_ctl::<Memory64>),
        "epoll_wait" => Function::new_typed_with_env(&mut store, env, epoll_wait::<Memory64>),
        "fd_advise" => Function::new_typed_with_env(&mut store, env, fd_advise),
        "fd_allocate" => Function::new_typed_with_env(&mut store, env, fd_allocate),
        "fd_close" => Function::new_typed_with_env(&mut store, env, fd_close),
        "fd_datasync" => Function::new_typed_with_env(&mut store, env, fd_datasync),
        "fd_fdstat_get" => Function::new_typed_with_env(&mut store, env, fd_fdstat_get::<Memory64>),
        "fd_fdstat_set_flags" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_flags),
        "fd_fdstat_set_rights" => Function::new_typed_with_env(&mut store, env, fd_fdstat_set_rights),
        "fd_filestat_get" => Function::new_typed_with_env(&mut store, env, fd_filestat_get::<Memory64>),
        "fd_filestat_set_size" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_size),
        "fd_filestat_set_times" => Function::new_typed_with_env(&mut store, env, fd_filestat_set_times),
        "fd_pread" => Function::new_typed_with_env(&mut store, env, fd_pread::<Memory64>),
        "fd_prestat_get" => Function::new_typed_with_env(&mut store, env, fd_prestat_get::<Memory64>),
        "fd_prestat_dir_name" => Function::new_typed_with_env(&mut store, env, fd_prestat_dir_name::<Memory64>),
        "fd_pwrite" => Function::new_typed_with_env(&mut store, env, fd_pwrite::<Memory64>),
        "fd_read" => Function::new_typed_with_env(&mut store, env, fd_read::<Memory64>),
        "fd_readdir" => Function::new_typed_with_env(&mut store, env, fd_readdir::<Memory64>),
        "fd_renumber" => Function::new_typed_with_env(&mut store, env, fd_renumber),
        "fd_dup" => Function::new_typed_with_env(&mut store, env, fd_dup::<Memory64>),
        "fd_dup2" => Function::new_typed_with_env(&mut store, env, fd_dup2::<Memory64>),
        "fd_fdflags_get" => Function::new_typed_with_env(&mut store, env, fd_fdflags_get::<Memory64>),
        "fd_fdflags_set" => Function::new_typed_with_env(&mut store, env, fd_fdflags_set),
        "fd_event" => Function::new_typed_with_env(&mut store, env, fd_event::<Memory64>),
        "fd_seek" => Function::new_typed_with_env(&mut store, env, fd_seek::<Memory64>),
        "fd_sync" => Function::new_typed_with_env(&mut store, env, fd_sync),
        "fd_tell" => Function::new_typed_with_env(&mut store, env, fd_tell::<Memory64>),
        "fd_write" => Function::new_typed_with_env(&mut store, env, fd_write::<Memory64>),
        "fd_pipe" => Function::new_typed_with_env(&mut store, env, fd_pipe::<Memory64>),
        "path_create_directory" => Function::new_typed_with_env(&mut store, env, path_create_directory::<Memory64>),
        "path_filestat_get" => Function::new_typed_with_env(&mut store, env, path_filestat_get::<Memory64>),
        "path_filestat_set_times" => Function::new_typed_with_env(&mut store, env, path_filestat_set_times::<Memory64>),
        "path_link" => Function::new_typed_with_env(&mut store, env, path_link::<Memory64>),
        "path_open" => Function::new_typed_with_env(&mut store, env, path_open::<Memory64>),
        "path_open2" => Function::new_typed_with_env(&mut store, env, path_open2::<Memory64>),
        "path_readlink" => Function::new_typed_with_env(&mut store, env, path_readlink::<Memory64>),
        "path_remove_directory" => Function::new_typed_with_env(&mut store, env, path_remove_directory::<Memory64>),
        "path_rename" => Function::new_typed_with_env(&mut store, env, path_rename::<Memory64>),
        "path_symlink" => Function::new_typed_with_env(&mut store, env, path_symlink::<Memory64>),
        "path_unlink_file" => Function::new_typed_with_env(&mut store, env, path_unlink_file::<Memory64>),
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, poll_oneoff::<Memory64>),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory64>),
        "proc_fork" => Function::new_typed_with_env(&mut store, env, proc_fork::<Memory64>),
        "proc_join" => Function::new_typed_with_env(&mut store, env, proc_join::<Memory64>),
        "proc_signal" => Function::new_typed_with_env(&mut store, env, proc_signal::<Memory64>),
        "proc_exec" => Function::new_typed_with_env(&mut store, env, proc_exec::<Memory64>),
        "proc_exec2" => Function::new_typed_with_env(&mut store, env, proc_exec2::<Memory64>),
        "proc_exec3" => Function::new_typed_with_env(&mut store, env, proc_exec3::<Memory64>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "proc_raise_interval" => Function::new_typed_with_env(&mut store, env, proc_raise_interval),
        "proc_snapshot" => Function::new_typed_with_env(&mut store, env, proc_snapshot::<Memory64>),
        "proc_spawn" => Function::new_typed_with_env(&mut store, env, proc_spawn::<Memory64>),
        "proc_id" => Function::new_typed_with_env(&mut store, env, proc_id::<Memory64>),
        "proc_parent" => Function::new_typed_with_env(&mut store, env, proc_parent::<Memory64>),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory64>),
        "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get::<Memory64>),
        "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set::<Memory64>),
        "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd::<Memory64>),
        "chdir" => Function::new_typed_with_env(&mut store, env, chdir::<Memory64>),
        "callback_signal" => Function::new_typed_with_env(&mut store, env, callback_signal::<Memory64>),
        "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn_v2::<Memory64>),
        "thread_spawn_v2" => Function::new_typed_with_env(&mut store, env, thread_spawn_v2::<Memory64>),
        "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep::<Memory64>),
        "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id::<Memory64>),
        "thread_signal" => Function::new_typed_with_env(&mut store, env, thread_signal),
        "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join::<Memory64>),
        "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism::<Memory64>),
        "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield::<Memory64>),
        "stack_checkpoint" => Function::new_typed_with_env(&mut store, env, stack_checkpoint::<Memory64>),
        "stack_restore" => Function::new_typed_with_env(&mut store, env, stack_restore::<Memory64>),
        "futex_wait" => Function::new_typed_with_env(&mut store, env, futex_wait::<Memory64>),
        "futex_wake" => Function::new_typed_with_env(&mut store, env, futex_wake::<Memory64>),
        "futex_wake_all" => Function::new_typed_with_env(&mut store, env, futex_wake_all::<Memory64>),
        "port_bridge" => Function::new_typed_with_env(&mut store, env, port_bridge::<Memory64>),
        "port_unbridge" => Function::new_typed_with_env(&mut store, env, port_unbridge),
        "port_dhcp_acquire" => Function::new_typed_with_env(&mut store, env, port_dhcp_acquire),
        "port_addr_add" => Function::new_typed_with_env(&mut store, env, port_addr_add::<Memory64>),
        "port_addr_remove" => Function::new_typed_with_env(&mut store, env, port_addr_remove::<Memory64>),
        "port_addr_clear" => Function::new_typed_with_env(&mut store, env, port_addr_clear),
        "port_addr_list" => Function::new_typed_with_env(&mut store, env, port_addr_list::<Memory64>),
        "port_mac" => Function::new_typed_with_env(&mut store, env, port_mac::<Memory64>),
        "port_gateway_set" => Function::new_typed_with_env(&mut store, env, port_gateway_set::<Memory64>),
        "port_route_add" => Function::new_typed_with_env(&mut store, env, port_route_add::<Memory64>),
        "port_route_remove" => Function::new_typed_with_env(&mut store, env, port_route_remove::<Memory64>),
        "port_route_clear" => Function::new_typed_with_env(&mut store, env, port_route_clear),
        "port_route_list" => Function::new_typed_with_env(&mut store, env, port_route_list::<Memory64>),
        "sock_status" => Function::new_typed_with_env(&mut store, env, sock_status::<Memory64>),
        "sock_addr_local" => Function::new_typed_with_env(&mut store, env, sock_addr_local::<Memory64>),
        "sock_addr_peer" => Function::new_typed_with_env(&mut store, env, sock_addr_peer::<Memory64>),
        "sock_open" => Function::new_typed_with_env(&mut store, env, sock_open::<Memory64>),
        "sock_pair" => Function::new_typed_with_env(&mut store, env, sock_pair::<Memory64>),
        "sock_set_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_set_opt_flag),
        "sock_get_opt_flag" => Function::new_typed_with_env(&mut store, env, sock_get_opt_flag::<Memory64>),
        "sock_set_opt_time" => Function::new_typed_with_env(&mut store, env, sock_set_opt_time::<Memory64>),
        "sock_get_opt_time" => Function::new_typed_with_env(&mut store, env, sock_get_opt_time::<Memory64>),
        "sock_set_opt_size" => Function::new_typed_with_env(&mut store, env, sock_set_opt_size),
        "sock_get_opt_size" => Function::new_typed_with_env(&mut store, env, sock_get_opt_size::<Memory64>),
        "sock_join_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v4::<Memory64>),
        "sock_leave_multicast_v4" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v4::<Memory64>),
        "sock_join_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_join_multicast_v6::<Memory64>),
        "sock_leave_multicast_v6" => Function::new_typed_with_env(&mut store, env, sock_leave_multicast_v6::<Memory64>),
        "sock_bind" => Function::new_typed_with_env(&mut store, env, sock_bind::<Memory64>),
        "sock_listen" => Function::new_typed_with_env(&mut store, env, sock_listen::<Memory64>),
        "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept_v2::<Memory64>),
        "sock_accept_v2" => Function::new_typed_with_env(&mut store, env, sock_accept_v2::<Memory64>),
        "sock_connect" => Function::new_typed_with_env(&mut store, env, sock_connect::<Memory64>),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory64>),
        "sock_recv_from" => Function::new_typed_with_env(&mut store, env, sock_recv_from::<Memory64>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory64>),
        "sock_send_to" => Function::new_typed_with_env(&mut store, env, sock_send_to::<Memory64>),
        "sock_send_file" => Function::new_typed_with_env(&mut store, env, sock_send_file::<Memory64>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
        "resolve" => Function::new_typed_with_env(&mut store, env, resolve::<Memory64>),
    };
    namespace
}

pub type InstanceInitializer =
    Box<dyn FnOnce(&wasmer::Instance, &dyn wasmer::AsStoreRef) -> Result<(), anyhow::Error>>;

type ModuleInitializer =
    Box<dyn FnOnce(&wasmer::Instance, &dyn wasmer::AsStoreRef) -> Result<(), anyhow::Error>>;

/// No-op module initializer.
fn stub_initializer(
    _instance: &wasmer::Instance,
    _store: &dyn wasmer::AsStoreRef,
) -> Result<(), anyhow::Error> {
    Ok(())
}

// TODO: split function into two variants, one for JS and one for sys.
// (this will make code less messy)
fn import_object_for_all_wasi_versions(
    _module: &wasmer::Module,
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> (Imports, ModuleInitializer) {
    let exports_wasi_generic = wasi_exports_generic(store, env);
    let exports_wasi_unstable = wasi_unstable_exports(store, env);
    let exports_wasi_snapshot_preview1 = wasi_snapshot_preview1_exports(store, env);
    let exports_wasix_32v1 = wasix_exports_32(store, env);
    let exports_wasix_64v1 = wasix_exports_64(store, env);

    // Allowed due to JS feature flag complications.
    #[allow(unused_mut)]
    let mut imports = imports! {
        "wasi" => exports_wasi_generic,
        "wasi_unstable" => exports_wasi_unstable,
        "wasi_snapshot_preview1" => exports_wasi_snapshot_preview1,
        "wasix_32v1" => exports_wasix_32v1,
        "wasix_64v1" => exports_wasix_64v1,
    };

    let init = Box::new(stub_initializer) as ModuleInitializer;

    (imports, init)
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_unstable = wasi_unstable_exports(store, env);
    imports! {
        "wasi_unstable" => exports_unstable
    }
}

fn generate_import_object_snapshot1(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasi_snapshot_preview1 = wasi_snapshot_preview1_exports(store, env);
    imports! {
        "wasi_snapshot_preview1" => exports_wasi_snapshot_preview1
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_wasix32_v1(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasix_32v1 = wasix_exports_32(store, env);
    imports! {
        "wasix_32v1" => exports_wasix_32v1
    }
}

fn generate_import_object_wasix64_v1(
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> Imports {
    let exports_wasix_64v1 = wasix_exports_64(store, env);
    imports! {
        "wasix_64v1" => exports_wasix_64v1
    }
}

fn mem_error_to_wasi(err: MemoryAccessError) -> Errno {
    match err {
        MemoryAccessError::HeapOutOfBounds => Errno::Memviolation,
        MemoryAccessError::Overflow => Errno::Overflow,
        MemoryAccessError::NonUtf8String => Errno::Inval,
        _ => Errno::Unknown,
    }
}

/// Run a synchronous function that would normally be blocking.
///
/// When the `sys-thread` feature is enabled, this will call
/// [`tokio::task::block_in_place()`]. Otherwise, it calls the function
/// immediately.
pub(crate) fn block_in_place<Ret>(thunk: impl FnOnce() -> Ret) -> Ret {
    cfg_if::cfg_if! {
        if #[cfg(feature = "sys-thread")] {
            tokio::task::block_in_place(thunk)
        } else {
            thunk()
        }
    }
}

/// Spawns a new blocking task that runs the provided closure.
///
/// The closure is executed on a separate thread, allowing it to perform blocking operations
/// without blocking the main thread. The closure is wrapped in a `Future` that resolves to the
/// result of the closure's execution.
pub(crate) async fn spawn_blocking<F, R>(f: F) -> Result<R, tokio::task::JoinError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            Ok(block_in_place(f))
        } else {
            tokio::task::spawn_blocking(f).await
        }
    }
}
