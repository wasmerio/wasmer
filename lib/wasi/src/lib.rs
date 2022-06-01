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

#[cfg(all(feature = "host-fs", feature = "mem-fs"))]
compile_error!(
    "Cannot have both `host-fs` and `mem-fs` features enabled at the same time. Please, pick one."
);

#[macro_use]
mod macros;
mod state;
mod syscalls;
mod utils;

use crate::syscalls::*;

pub use crate::state::{
    Fd, Pipe, Stderr, Stdin, Stdout, WasiFs, WasiState, WasiStateBuilder, WasiStateCreationError,
    ALL_RIGHTS, VIRTUAL_ROOT_FD,
};
pub use crate::syscalls::types;
pub use crate::utils::{get_wasi_version, get_wasi_versions, is_wasi_module, WasiVersion};
use wasmer::ContextMut;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::FsError`")]
pub use wasmer_vfs::FsError as WasiFsError;
#[deprecated(since = "2.1.0", note = "Please use `wasmer_vfs::VirtualFile`")]
pub use wasmer_vfs::VirtualFile as WasiFile;
pub use wasmer_vfs::{FsError, VirtualFile};

use thiserror::Error;
use wasmer::{imports, Function, Imports, Memory, MemoryAccessError};

use std::sync::{Arc, Mutex, MutexGuard};

/// This is returned in `RuntimeError`.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
#[derive(Error, Debug)]
pub enum WasiError {
    #[error("WASI exited with code: {0}")]
    Exit(syscalls::types::__wasi_exitcode_t),
    #[error("The WASI version could not be determined")]
    UnknownWasiVersion,
}

/// The environment provided to the WASI imports.
#[derive(Debug, Clone)]
pub struct WasiEnv {
    /// Shared state of the WASI system. Manages all the data that the
    /// executing WASI program can see.
    pub state: Arc<Mutex<WasiState>>,
    /// The memory of the Wasm object
    memory: Option<Memory>,
}

impl WasiEnv {
    /// Create a new WasiEnv from a WasiState (memory will be set to None)
    pub fn new(state: WasiState) -> Self {
        WasiEnv {
            state: Arc::new(Mutex::new(state)),
            memory: None,
        }
    }
    /// Set the memory of the WasiEnv (can only be done once)
    pub fn set_memory(&mut self, memory: Memory) {
        if !self.memory.is_none() {
            panic!("Memory of a WasiEnv can only be set once!");
        }
        self.memory = Some(memory);
    }
    /// Get memory, that needs to have been set fist
    pub fn memory(&self) -> &Memory {
        self.memory.as_ref().unwrap()
    }
    /// Get the WASI state
    pub fn state(&self) -> MutexGuard<WasiState> {
        self.state.lock().unwrap()
    }
    /// Get both WasiState and Memory from the WasiEnv
    pub fn get_memory_and_wasi_state(&self, _mem_index: u32) -> (&Memory, MutexGuard<WasiState>) {
        let memory = self.memory();
        let state = self.state.lock().unwrap();
        (memory, state)
    }
}

/// Create an [`Imports`]  from a [`Context`]
pub fn generate_import_object_from_ctx(
    ctx: &mut ContextMut<'_, WasiEnv>,
    version: WasiVersion,
) -> Imports {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(ctx),
        WasiVersion::Snapshot1 | WasiVersion::Latest => generate_import_object_snapshot1(ctx),
    }
}

pub fn import_object_for_all_wasi_versions(ctx: &mut ContextMut<'_, WasiEnv>) -> Imports {
    imports! {
        "wasi_unstable" => {
            "args_get" => Function::new_native(ctx, args_get),
            "args_sizes_get" => Function::new_native(ctx, args_sizes_get),
            "clock_res_get" => Function::new_native(ctx, clock_res_get),
            "clock_time_get" => Function::new_native(ctx, clock_time_get),
            "environ_get" => Function::new_native(ctx, environ_get),
            "environ_sizes_get" => Function::new_native(ctx, environ_sizes_get),
            "fd_advise" => Function::new_native(ctx, fd_advise),
            "fd_allocate" => Function::new_native(ctx, fd_allocate),
            "fd_close" => Function::new_native(ctx, fd_close),
            "fd_datasync" => Function::new_native(ctx, fd_datasync),
            "fd_fdstat_get" => Function::new_native(ctx, fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native(ctx, fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native(ctx, fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native(ctx, legacy::snapshot0::fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native(ctx, fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native(ctx, fd_filestat_set_times),
            "fd_pread" => Function::new_native(ctx, fd_pread),
            "fd_prestat_get" => Function::new_native(ctx, fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native(ctx, fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native(ctx, fd_pwrite),
            "fd_read" => Function::new_native(ctx, fd_read),
            "fd_readdir" => Function::new_native(ctx, fd_readdir),
            "fd_renumber" => Function::new_native(ctx, fd_renumber),
            "fd_seek" => Function::new_native(ctx, legacy::snapshot0::fd_seek),
            "fd_sync" => Function::new_native(ctx, fd_sync),
            "fd_tell" => Function::new_native(ctx, fd_tell),
            "fd_write" => Function::new_native(ctx, fd_write),
            "path_create_directory" => Function::new_native(ctx, path_create_directory),
            "path_filestat_get" => Function::new_native(ctx, legacy::snapshot0::path_filestat_get),
            "path_filestat_set_times" => Function::new_native(ctx, path_filestat_set_times),
            "path_link" => Function::new_native(ctx, path_link),
            "path_open" => Function::new_native(ctx, path_open),
            "path_readlink" => Function::new_native(ctx, path_readlink),
            "path_remove_directory" => Function::new_native(ctx, path_remove_directory),
            "path_rename" => Function::new_native(ctx, path_rename),
            "path_symlink" => Function::new_native(ctx, path_symlink),
            "path_unlink_file" => Function::new_native(ctx, path_unlink_file),
            "poll_oneoff" => Function::new_native(ctx, legacy::snapshot0::poll_oneoff),
            "proc_exit" => Function::new_native(ctx, proc_exit),
            "proc_raise" => Function::new_native(ctx, proc_raise),
            "random_get" => Function::new_native(ctx, random_get),
            "sched_yield" => Function::new_native(ctx, sched_yield),
            "sock_recv" => Function::new_native(ctx, sock_recv),
            "sock_send" => Function::new_native(ctx, sock_send),
            "sock_shutdown" => Function::new_native(ctx, sock_shutdown),
        },
        "wasi_snapshot_preview1" => {
            "args_get" => Function::new_native(ctx, args_get),
            "args_sizes_get" => Function::new_native(ctx, args_sizes_get),
            "clock_res_get" => Function::new_native(ctx, clock_res_get),
            "clock_time_get" => Function::new_native(ctx, clock_time_get),
            "environ_get" => Function::new_native(ctx, environ_get),
            "environ_sizes_get" => Function::new_native(ctx, environ_sizes_get),
            "fd_advise" => Function::new_native(ctx, fd_advise),
            "fd_allocate" => Function::new_native(ctx, fd_allocate),
            "fd_close" => Function::new_native(ctx, fd_close),
            "fd_datasync" => Function::new_native(ctx, fd_datasync),
            "fd_fdstat_get" => Function::new_native(ctx, fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native(ctx, fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native(ctx, fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native(ctx, fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native(ctx, fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native(ctx, fd_filestat_set_times),
            "fd_pread" => Function::new_native(ctx, fd_pread),
            "fd_prestat_get" => Function::new_native(ctx, fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native(ctx, fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native(ctx, fd_pwrite),
            "fd_read" => Function::new_native(ctx, fd_read),
            "fd_readdir" => Function::new_native(ctx, fd_readdir),
            "fd_renumber" => Function::new_native(ctx, fd_renumber),
            "fd_seek" => Function::new_native(ctx, fd_seek),
            "fd_sync" => Function::new_native(ctx, fd_sync),
            "fd_tell" => Function::new_native(ctx, fd_tell),
            "fd_write" => Function::new_native(ctx, fd_write),
            "path_create_directory" => Function::new_native(ctx, path_create_directory),
            "path_filestat_get" => Function::new_native(ctx, path_filestat_get),
            "path_filestat_set_times" => Function::new_native(ctx, path_filestat_set_times),
            "path_link" => Function::new_native(ctx, path_link),
            "path_open" => Function::new_native(ctx, path_open),
            "path_readlink" => Function::new_native(ctx, path_readlink),
            "path_remove_directory" => Function::new_native(ctx, path_remove_directory),
            "path_rename" => Function::new_native(ctx, path_rename),
            "path_symlink" => Function::new_native(ctx, path_symlink),
            "path_unlink_file" => Function::new_native(ctx, path_unlink_file),
            "poll_oneoff" => Function::new_native(ctx, poll_oneoff),
            "proc_exit" => Function::new_native(ctx, proc_exit),
            "proc_raise" => Function::new_native(ctx, proc_raise),
            "random_get" => Function::new_native(ctx, random_get),
            "sched_yield" => Function::new_native(ctx, sched_yield),
            "sock_recv" => Function::new_native(ctx, sock_recv),
            "sock_send" => Function::new_native(ctx, sock_send),
            "sock_shutdown" => Function::new_native(ctx, sock_shutdown),
        }
    }
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(ctx: &mut ContextMut<'_, WasiEnv>) -> Imports {
    imports! {
        "wasi_unstable" => {
            "args_get" => Function::new_native(ctx, args_get),
            "args_sizes_get" => Function::new_native(ctx, args_sizes_get),
            "clock_res_get" => Function::new_native(ctx, clock_res_get),
            "clock_time_get" => Function::new_native(ctx, clock_time_get),
            "environ_get" => Function::new_native(ctx, environ_get),
            "environ_sizes_get" => Function::new_native(ctx, environ_sizes_get),
            "fd_advise" => Function::new_native(ctx, fd_advise),
            "fd_allocate" => Function::new_native(ctx, fd_allocate),
            "fd_close" => Function::new_native(ctx, fd_close),
            "fd_datasync" => Function::new_native(ctx, fd_datasync),
            "fd_fdstat_get" => Function::new_native(ctx, fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native(ctx, fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native(ctx, fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native(ctx, legacy::snapshot0::fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native(ctx, fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native(ctx, fd_filestat_set_times),
            "fd_pread" => Function::new_native(ctx, fd_pread),
            "fd_prestat_get" => Function::new_native(ctx, fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native(ctx, fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native(ctx, fd_pwrite),
            "fd_read" => Function::new_native(ctx, fd_read),
            "fd_readdir" => Function::new_native(ctx, fd_readdir),
            "fd_renumber" => Function::new_native(ctx, fd_renumber),
            "fd_seek" => Function::new_native(ctx, legacy::snapshot0::fd_seek),
            "fd_sync" => Function::new_native(ctx, fd_sync),
            "fd_tell" => Function::new_native(ctx, fd_tell),
            "fd_write" => Function::new_native(ctx, fd_write),
            "path_create_directory" => Function::new_native(ctx, path_create_directory),
            "path_filestat_get" => Function::new_native(ctx, legacy::snapshot0::path_filestat_get),
            "path_filestat_set_times" => Function::new_native(ctx, path_filestat_set_times),
            "path_link" => Function::new_native(ctx, path_link),
            "path_open" => Function::new_native(ctx, path_open),
            "path_readlink" => Function::new_native(ctx, path_readlink),
            "path_remove_directory" => Function::new_native(ctx, path_remove_directory),
            "path_rename" => Function::new_native(ctx, path_rename),
            "path_symlink" => Function::new_native(ctx, path_symlink),
            "path_unlink_file" => Function::new_native(ctx, path_unlink_file),
            "poll_oneoff" => Function::new_native(ctx, legacy::snapshot0::poll_oneoff),
            "proc_exit" => Function::new_native(ctx, proc_exit),
            "proc_raise" => Function::new_native(ctx, proc_raise),
            "random_get" => Function::new_native(ctx, random_get),
            "sched_yield" => Function::new_native(ctx, sched_yield),
            "sock_recv" => Function::new_native(ctx, sock_recv),
            "sock_send" => Function::new_native(ctx, sock_send),
            "sock_shutdown" => Function::new_native(ctx, sock_shutdown),
        },
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_snapshot1(ctx: &mut ContextMut<'_, WasiEnv>) -> Imports {
    imports! {
        "wasi_snapshot_preview1" => {
            "args_get" => Function::new_native(ctx, args_get),
            "args_sizes_get" => Function::new_native(ctx, args_sizes_get),
            "clock_res_get" => Function::new_native(ctx, clock_res_get),
            "clock_time_get" => Function::new_native(ctx, clock_time_get),
            "environ_get" => Function::new_native(ctx, environ_get),
            "environ_sizes_get" => Function::new_native(ctx, environ_sizes_get),
            "fd_advise" => Function::new_native(ctx, fd_advise),
            "fd_allocate" => Function::new_native(ctx, fd_allocate),
            "fd_close" => Function::new_native(ctx, fd_close),
            "fd_datasync" => Function::new_native(ctx, fd_datasync),
            "fd_fdstat_get" => Function::new_native(ctx, fd_fdstat_get),
            "fd_fdstat_set_flags" => Function::new_native(ctx, fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => Function::new_native(ctx, fd_fdstat_set_rights),
            "fd_filestat_get" => Function::new_native(ctx, fd_filestat_get),
            "fd_filestat_set_size" => Function::new_native(ctx, fd_filestat_set_size),
            "fd_filestat_set_times" => Function::new_native(ctx, fd_filestat_set_times),
            "fd_pread" => Function::new_native(ctx, fd_pread),
            "fd_prestat_get" => Function::new_native(ctx, fd_prestat_get),
            "fd_prestat_dir_name" => Function::new_native(ctx, fd_prestat_dir_name),
            "fd_pwrite" => Function::new_native(ctx, fd_pwrite),
            "fd_read" => Function::new_native(ctx, fd_read),
            "fd_readdir" => Function::new_native(ctx, fd_readdir),
            "fd_renumber" => Function::new_native(ctx, fd_renumber),
            "fd_seek" => Function::new_native(ctx, fd_seek),
            "fd_sync" => Function::new_native(ctx, fd_sync),
            "fd_tell" => Function::new_native(ctx, fd_tell),
            "fd_write" => Function::new_native(ctx, fd_write),
            "path_create_directory" => Function::new_native(ctx, path_create_directory),
            "path_filestat_get" => Function::new_native(ctx, path_filestat_get),
            "path_filestat_set_times" => Function::new_native(ctx, path_filestat_set_times),
            "path_link" => Function::new_native(ctx, path_link),
            "path_open" => Function::new_native(ctx, path_open),
            "path_readlink" => Function::new_native(ctx, path_readlink),
            "path_remove_directory" => Function::new_native(ctx, path_remove_directory),
            "path_rename" => Function::new_native(ctx, path_rename),
            "path_symlink" => Function::new_native(ctx, path_symlink),
            "path_unlink_file" => Function::new_native(ctx, path_unlink_file),
            "poll_oneoff" => Function::new_native(ctx, poll_oneoff),
            "proc_exit" => Function::new_native(ctx, proc_exit),
            "proc_raise" => Function::new_native(ctx, proc_raise),
            "random_get" => Function::new_native(ctx, random_get),
            "sched_yield" => Function::new_native(ctx, sched_yield),
            "sock_recv" => Function::new_native(ctx, sock_recv),
            "sock_send" => Function::new_native(ctx, sock_send),
            "sock_shutdown" => Function::new_native(ctx, sock_shutdown),
        }
    }
}

fn mem_error_to_wasi(err: MemoryAccessError) -> types::__wasi_errno_t {
    match err {
        MemoryAccessError::HeapOutOfBounds => types::__WASI_EFAULT,
        MemoryAccessError::Overflow => types::__WASI_EOVERFLOW,
        MemoryAccessError::NonUtf8String => types::__WASI_EINVAL,
        _ => types::__WASI_EINVAL,
    }
}
