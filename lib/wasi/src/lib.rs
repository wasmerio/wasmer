// FIXME: merge with ./lib.rs_upstream

#![deny(unused_mut)]
#![doc(html_favicon_url = "https://wasmer.io/images/icons/favicon-32x32.png")]
#![doc(html_logo_url = "https://github.com/wasmerio.png?size=200")]

//! Wasmer's WASI implementation
//!
//! Use `generate_import_object` to create an [`Imports`].  This [`Imports`]
//! can be combined with a module to create an `Instance` which can execute WASI
//! Wasm functions.
//!
//! See `state` for the experimental WASI FS API.  Also see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/master/examples/plugin.rs)
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
    "The `js` feature must be enabled only for the `wasm32` target (either `wasm32-unknown-unknown` or `wasm32-wasi`)."
);

#[macro_use]
mod macros;
pub mod bin_factory;
pub mod os;
// TODO: should this be pub?
pub mod net;
// TODO: should this be pub?
pub mod fs;
pub mod http;
#[cfg(feature = "webc_runner")]
pub mod runners;
pub mod runtime;
mod state;
mod syscalls;
mod utils;
pub mod wapm;

/// WAI based bindings.
mod bindings;

use std::sync::Arc;
use std::{
    cell::RefCell,
    sync::atomic::{AtomicU32, Ordering},
};

#[allow(unused_imports)]
use bytes::{Bytes, BytesMut};
use os::task::control_plane::ControlPlaneError;
use thiserror::Error;
use tracing::error;
// re-exports needed for OS
pub use wasmer;
pub use wasmer_wasi_types;

use wasmer::{
    imports, namespace, AsStoreMut, Exports, FunctionEnv, Imports, Memory32, MemoryAccessError,
    MemorySize, RuntimeError,
};

pub use wasmer_vfs;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::FsError`")]
pub use wasmer_vfs::FsError as WasiFsError;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::VirtualFile`")]
pub use wasmer_vfs::VirtualFile as WasiFile;
pub use wasmer_vfs::{DuplexPipe, FsError, Pipe, VirtualFile, WasiBidirectionalSharedPipePair};
pub use wasmer_vnet;
pub use wasmer_vnet::{UnsupportedVirtualNetworking, VirtualNetworking};

#[cfg(feature = "host-vnet")]
pub use wasmer_wasi_local_networking::{
    io_err_into_net_error, LocalNetworking, LocalTcpListener, LocalTcpStream, LocalUdpSocket,
};
use wasmer_wasi_types::wasi::{BusErrno, Errno, ExitCode};

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
    runtime::{
        task_manager::{VirtualTaskManager, VirtualTaskManagerExt},
        PluggableRuntimeImplementation, SpawnedMemory, WasiRuntime,
    },
    wapm::parse_static_webc,
};

pub use crate::utils::is_wasix_module;

pub use crate::{
    state::{
        Capabilities, WasiEnv, WasiEnvBuilder, WasiEnvInit, WasiFunctionEnv, WasiInstanceHandles,
        WasiStateCreationError, ALL_RIGHTS,
    },
    syscalls::types,
    utils::{get_wasi_version, get_wasi_versions, is_wasi_module, WasiVersion},
};

/// This is returned in `RuntimeError`.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
#[derive(Error, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WasiError {
    #[error("WASI exited with code: {0}")]
    Exit(ExitCode),
    #[error("The WASI version could not be determined")]
    UnknownWasiVersion,
}

/// Represents the ID of a WASI calling thread
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiCallingId(u32);

impl WasiCallingId {
    pub fn raw(&self) -> u32 {
        self.0
    }

    pub fn inc(&mut self) -> WasiCallingId {
        self.0 += 1;
        *self
    }
}

impl From<u32> for WasiCallingId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl From<WasiCallingId> for u32 {
    fn from(t: WasiCallingId) -> u32 {
        t.0 as u32
    }
}

/// The default stack size for WASIX
pub const DEFAULT_STACK_SIZE: u64 = 1_048_576u64;
pub const DEFAULT_STACK_BASE: u64 = DEFAULT_STACK_SIZE;

// TODO: remove, this is a leftover from an old vbus crate and should be folded
// into WasiRuntimeError.
#[derive(Error, Copy, Clone, Debug, PartialEq, Eq)]
pub enum VirtualBusError {
    /// Failed during serialization
    #[error("serialization failed")]
    Serialization,
    /// Failed during deserialization
    #[error("deserialization failed")]
    Deserialization,
    /// Invalid WAPM process
    #[error("invalid wapm")]
    InvalidWapm,
    /// Failed to fetch the WAPM process
    #[error("fetch failed")]
    FetchFailed,
    /// Failed to compile the WAPM process
    #[error("compile error")]
    CompileError,
    /// Invalid ABI
    #[error("WAPM process has an invalid ABI")]
    InvalidABI,
    /// Call was aborted
    #[error("call aborted")]
    Aborted,
    /// Bad handle
    #[error("bad handle")]
    BadHandle,
    /// Invalid topic
    #[error("invalid topic")]
    InvalidTopic,
    /// Invalid callback
    #[error("invalid callback")]
    BadCallback,
    /// Call is unsupported
    #[error("unsupported")]
    Unsupported,
    /// Not found
    #[error("not found")]
    NotFound,
    /// Bad request
    #[error("bad request")]
    BadRequest,
    /// Access denied
    #[error("access denied")]
    AccessDenied,
    /// Internal error has occured
    #[error("internal error")]
    InternalError,
    /// Memory allocation failed
    #[error("memory allocation failed")]
    MemoryAllocationFailed,
    /// Invocation has failed
    #[error("invocation has failed")]
    InvokeFailed,
    /// Already consumed
    #[error("already consumed")]
    AlreadyConsumed,
    /// Memory access violation
    #[error("memory access violation")]
    MemoryAccessViolation,
    /// Some other unhandled error. If you see this, it's probably a bug.
    #[error("unknown error found")]
    UnknownError,
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
    #[error("Runtime error")]
    Runtime(#[from] RuntimeError),
    #[error("Memory access error")]
    Thread(#[from] WasiThreadError),
}

impl WasiRuntimeError {
    /// Retrieve the concrete exit code returned by an instance.
    ///
    /// Returns [`None`] if a general execution error ocurred.
    pub fn as_exit_code(&self) -> Option<ExitCode> {
        if let WasiRuntimeError::Wasi(WasiError::Exit(code)) = self {
            Some(*code)
        } else {
            None
        }
    }
}

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
    /// The memory stack before the vfork occured
    pub memory_stack: BytesMut,
    /// The mutable parts of the store
    pub store_data: Bytes,
    /// Offset into the memory where the PID will be
    /// written when the real fork takes places
    pub pid_offset: u64,

    /// The environment before the vfork occured
    pub env: Box<WasiEnv>,

    /// Handle of the thread we have forked (dropping this handle
    /// will signal that the thread is dead)
    pub handle: WasiThreadHandle,
}

impl WasiVFork {
    /// Clones this env.
    ///
    /// This is a custom function instead of a [`Clone`] implementation because
    /// this type should not be cloned.
    ///
    // TODO: remove WasiEnv::duplicate()
    // This function should not exist, since it just copies internal state.
    // Currently only used by fork/spawn related syscalls.
    pub(crate) fn duplicate(&self) -> Self {
        Self {
            rewind_stack: self.rewind_stack.clone(),
            memory_stack: self.memory_stack.clone(),
            store_data: self.store_data.clone(),
            pid_offset: self.pid_offset,
            env: Box::new(self.env.duplicate()),
            handle: self.handle.clone(),
        }
    }
}

// Represents the current thread ID for the executing method
thread_local!(pub(crate) static CALLER_ID: RefCell<u32> = RefCell::new(0));
thread_local!(pub(crate) static REWIND: RefCell<Option<bytes::Bytes>> = RefCell::new(None));
lazy_static::lazy_static! {
    static ref CALLER_ID_SEED: Arc<AtomicU32> = Arc::new(AtomicU32::new(1));
}

/// Returns the current thread ID
pub fn current_caller_id() -> WasiCallingId {
    CALLER_ID
        .with(|f| {
            let mut caller_id = f.borrow_mut();
            if *caller_id == 0 {
                *caller_id = CALLER_ID_SEED.fetch_add(1, Ordering::AcqRel);
            }
            *caller_id
        })
        .into()
}

/// Create an [`Imports`] with an existing [`WasiEnv`]. `WasiEnv`
/// needs a [`WasiState`], that can be constructed from a
/// [`WasiEnvBuilder`](state::WasiEnvBuilder).
pub fn generate_import_object_from_env(
    store: &mut impl AsStoreMut,
    ctx: &FunctionEnv<WasiEnv>,
    version: WasiVersion,
) -> Imports {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(store, ctx),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(store, ctx)
        }
        WasiVersion::Wasix32v1 => generate_import_object_wasix32_v1(store, ctx),
        WasiVersion::Wasix64v1 => generate_import_object_wasix64_v1(store, ctx),
    }
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
        "poll_oneoff" => Function::new_typed_with_env(&mut store, env, legacy::snapshot0::poll_oneoff),
        "proc_exit" => Function::new_typed_with_env(&mut store, env, proc_exit::<Memory32>),
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
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
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
        "sock_recv" => Function::new_typed_with_env(&mut store, env, sock_recv::<Memory32>),
        "sock_send" => Function::new_typed_with_env(&mut store, env, sock_send::<Memory32>),
        "sock_shutdown" => Function::new_typed_with_env(&mut store, env, sock_shutdown),
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
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "proc_raise_interval" => Function::new_typed_with_env(&mut store, env, proc_raise_interval),
        "proc_spawn" => Function::new_typed_with_env(&mut store, env, proc_spawn::<Memory32>),
        "proc_id" => Function::new_typed_with_env(&mut store, env, proc_id::<Memory32>),
        "proc_parent" => Function::new_typed_with_env(&mut store, env, proc_parent::<Memory32>),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory32>),
        "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get::<Memory32>),
        "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set::<Memory32>),
        "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd::<Memory32>),
        "chdir" => Function::new_typed_with_env(&mut store, env, chdir::<Memory32>),
        "callback_signal" => Function::new_typed_with_env(&mut store, env, callback_signal::<Memory32>),
        "callback_thread" => Function::new_typed_with_env(&mut store, env, callback_thread::<Memory32>),
        "callback_reactor" => Function::new_typed_with_env(&mut store, env, callback_reactor::<Memory32>),
        "callback_thread_local_destroy" => Function::new_typed_with_env(&mut store, env, callback_thread_local_destroy::<Memory32>),
        "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn::<Memory32>),
        "thread_local_create" => Function::new_typed_with_env(&mut store, env, thread_local_create::<Memory32>),
        "thread_local_destroy" => Function::new_typed_with_env(&mut store, env, thread_local_destroy),
        "thread_local_set" => Function::new_typed_with_env(&mut store, env, thread_local_set),
        "thread_local_get" => Function::new_typed_with_env(&mut store, env, thread_local_get::<Memory32>),
        "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep),
        "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id::<Memory32>),
        "thread_signal" => Function::new_typed_with_env(&mut store, env, thread_signal),
        "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join),
        "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism::<Memory32>),
        "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
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
        "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept::<Memory32>),
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
        "proc_raise" => Function::new_typed_with_env(&mut store, env, proc_raise),
        "proc_raise_interval" => Function::new_typed_with_env(&mut store, env, proc_raise_interval),
        "proc_spawn" => Function::new_typed_with_env(&mut store, env, proc_spawn::<Memory64>),
        "proc_id" => Function::new_typed_with_env(&mut store, env, proc_id::<Memory64>),
        "proc_parent" => Function::new_typed_with_env(&mut store, env, proc_parent::<Memory64>),
        "random_get" => Function::new_typed_with_env(&mut store, env, random_get::<Memory64>),
        "tty_get" => Function::new_typed_with_env(&mut store, env, tty_get::<Memory64>),
        "tty_set" => Function::new_typed_with_env(&mut store, env, tty_set::<Memory64>),
        "getcwd" => Function::new_typed_with_env(&mut store, env, getcwd::<Memory64>),
        "chdir" => Function::new_typed_with_env(&mut store, env, chdir::<Memory64>),
        "callback_signal" => Function::new_typed_with_env(&mut store, env, callback_signal::<Memory64>),
        "callback_thread" => Function::new_typed_with_env(&mut store, env, callback_thread::<Memory64>),
        "callback_reactor" => Function::new_typed_with_env(&mut store, env, callback_reactor::<Memory64>),
        "callback_thread_local_destroy" => Function::new_typed_with_env(&mut store, env, callback_thread_local_destroy::<Memory64>),
        "thread_spawn" => Function::new_typed_with_env(&mut store, env, thread_spawn::<Memory64>),
        "thread_local_create" => Function::new_typed_with_env(&mut store, env, thread_local_create::<Memory64>),
        "thread_local_destroy" => Function::new_typed_with_env(&mut store, env, thread_local_destroy),
        "thread_local_set" => Function::new_typed_with_env(&mut store, env, thread_local_set),
        "thread_local_get" => Function::new_typed_with_env(&mut store, env, thread_local_get::<Memory64>),
        "thread_sleep" => Function::new_typed_with_env(&mut store, env, thread_sleep),
        "thread_id" => Function::new_typed_with_env(&mut store, env, thread_id::<Memory64>),
        "thread_signal" => Function::new_typed_with_env(&mut store, env, thread_signal),
        "thread_join" => Function::new_typed_with_env(&mut store, env, thread_join),
        "thread_parallelism" => Function::new_typed_with_env(&mut store, env, thread_parallelism::<Memory64>),
        "thread_exit" => Function::new_typed_with_env(&mut store, env, thread_exit),
        "sched_yield" => Function::new_typed_with_env(&mut store, env, sched_yield),
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
        "sock_accept" => Function::new_typed_with_env(&mut store, env, sock_accept::<Memory64>),
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
    module: &wasmer::Module,
    store: &mut impl AsStoreMut,
    env: &FunctionEnv<WasiEnv>,
) -> (Imports, ModuleInitializer) {
    let exports_wasi_unstable = wasi_unstable_exports(store, env);
    let exports_wasi_snapshot_preview1 = wasi_snapshot_preview1_exports(store, env);
    let exports_wasix_32v1 = wasix_exports_32(store, env);
    let exports_wasix_64v1 = wasix_exports_64(store, env);

    // Allowed due to JS feature flag complications.
    #[allow(unused_mut)]
    let mut imports = imports! {
        "wasi_unstable" => exports_wasi_unstable,
        "wasi_snapshot_preview1" => exports_wasi_snapshot_preview1,
        "wasix_32v1" => exports_wasix_32v1,
        "wasix_64v1" => exports_wasix_64v1,
    };

    // TODO: clean this up!
    cfg_if::cfg_if! {
        if #[cfg(feature = "sys")] {
            // Check if the module needs http.

            let has_canonical_realloc = module.exports().any(|t| t.name() == "canonical_abi_realloc");
            let has_wasix_http_import = module.imports().any(|t| t.module() == "wasix_http_client_v1");

            let init = if has_canonical_realloc && has_wasix_http_import {
                let wenv = env.as_ref(store);
                let http = crate::http::client_impl::WasixHttpClientImpl::new(wenv);
                    crate::bindings::wasix_http_client_v1::add_to_imports(
                        store,
                        &mut imports,
                        http,
                    )
            } else {
                Box::new(stub_initializer) as ModuleInitializer
            };

            let init = init;
        } else {
            // Prevents unused warning.
            let _ = module;
            let init = Box::new(stub_initializer) as ModuleInitializer;
        }
    }

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
        MemoryAccessError::HeapOutOfBounds => Errno::Fault,
        MemoryAccessError::Overflow => Errno::Overflow,
        MemoryAccessError::NonUtf8String => Errno::Inval,
        _ => Errno::Inval,
    }
}

fn mem_error_to_bus(err: MemoryAccessError) -> BusErrno {
    match err {
        MemoryAccessError::HeapOutOfBounds => BusErrno::Memviolation,
        MemoryAccessError::Overflow => BusErrno::Memviolation,
        MemoryAccessError::NonUtf8String => BusErrno::Badrequest,
        _ => BusErrno::Unknown,
    }
}

#[cfg(all(feature = "sys"))]
pub fn build_test_engine(features: Option<wasmer::Features>) -> wasmer::Engine {
    let _ = features;
    wasmer::Store::default().engine().cloned()
}
