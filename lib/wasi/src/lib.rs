#![deny(unused_mut)]
#![doc(html_favicon_url = "https://wasmer.io/static/icons/favicon.ico")]
#![doc(html_logo_url = "https://avatars3.githubusercontent.com/u/44205449?s=200&v=4")]

//! Wasmer's WASI implementation
//!
//! Use `generate_import_object` to create an [`ImportObject`].  This [`ImportObject`]
//! can be combined with a module to create an `Instance` which can execute WASI
//! Wasm functions.
//!
//! See `state` for the experimental WASI FS API.  Also see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/master/examples/plugin.rs)
//! for an example of how to extend WASI using the WASI FS API.

#[cfg(target = "windows")]
extern crate winapi;
#[macro_use]
extern crate log;

#[macro_use]
mod macros;
mod ptr;
pub mod state;
mod syscalls;
mod utils;

use self::state::{WasiFs, WasiState};
pub use self::syscalls::types;
use self::syscalls::*;

pub use self::utils::{get_wasi_version, is_wasi_module, WasiVersion};

use wasmer::{imports, Func, ImportObject, Memory, Store};

/// This is returned in `RuntimeError`.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
pub struct ExitCode {
    pub code: syscalls::types::__wasi_exitcode_t,
}

/// The environment provided to the WASI imports.
/// It
pub struct WasiEnv<'a> {
    state: WasiState,
    memory: Option<&'a Memory>,
}

impl<'a> WasiEnv<'a> {
    pub fn new(state: WasiState) -> Self {
        Self {
            state,
            memory: None,
        }
    }

    /// Set the state
    pub fn set_memory(&mut self, memory: &'a Memory) {
        self.memory = Some(memory);
    }

    /// Get the WASI state
    pub fn state(&self) -> &WasiState {
        &self.state
    }

    /// Get the WASI state (mutable)
    pub fn state_mut(&mut self) -> &mut WasiState {
        &mut self.state
    }

    /// Get a reference to the memory
    pub fn memory(&self) -> &Memory {
        &self.memory.as_ref().unwrap()
    }
}

/// Create an [`ImportObject`] with an existing [`WasiState`]. [`WasiState`]
/// can be constructed from a [`WasiStateBuilder`](state::WasiStateBuilder).
pub fn generate_import_object_from_env(
    store: &Store,
    mut wasi_env: &mut WasiEnv,
    version: WasiVersion,
) -> ImportObject {
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0(&store, &mut wasi_env),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1(&store, &mut wasi_env)
        }
    }
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0(store: &Store, env: &mut WasiEnv) -> ImportObject {
    imports! {
        // "wasi_unstable" => {
        //     "args_get" => Func::new_env(&store, &mut env, args_get),
        //     "args_sizes_get" => Func::new_env(&store, &mut env, args_sizes_get),
        //     "clock_res_get" => Func::new_env(&store, &mut env, clock_res_get),
        //     "clock_time_get" => Func::new_env(&store, &mut env, clock_time_get),
        //     "environ_get" => Func::new_env(&store, &mut env, environ_get),
        //     "environ_sizes_get" => Func::new_env(&store, &mut env, environ_sizes_get),
        //     "fd_advise" => Func::new_env(&store, &mut env, fd_advise),
        //     "fd_allocate" => Func::new_env(&store, &mut env, fd_allocate),
        //     "fd_close" => Func::new_env(&store, &mut env, fd_close),
        //     "fd_datasync" => Func::new_env(&store, &mut env, fd_datasync),
        //     "fd_fdstat_get" => Func::new_env(&store, &mut env, fd_fdstat_get),
        //     "fd_fdstat_set_flags" => Func::new_env(&store, &mut env, fd_fdstat_set_flags),
        //     "fd_fdstat_set_rights" => Func::new_env(&store, &mut env, fd_fdstat_set_rights),
        //     "fd_filestat_get" => Func::new_env(&store, &mut env, legacy::snapshot0::fd_filestat_get),
        //     "fd_filestat_set_size" => Func::new_env(&store, &mut env, fd_filestat_set_size),
        //     "fd_filestat_set_times" => Func::new_env(&store, &mut env, fd_filestat_set_times),
        //     "fd_pread" => Func::new_env(&store, &mut env, fd_pread),
        //     "fd_prestat_get" => Func::new_env(&store, &mut env, fd_prestat_get),
        //     "fd_prestat_dir_name" => Func::new_env(&store, &mut env, fd_prestat_dir_name),
        //     "fd_pwrite" => Func::new_env(&store, &mut env, fd_pwrite),
        //     "fd_read" => Func::new_env(&store, &mut env, fd_read),
        //     "fd_readdir" => Func::new_env(&store, &mut env, fd_readdir),
        //     "fd_renumber" => Func::new_env(&store, &mut env, fd_renumber),
        //     "fd_seek" => Func::new_env(&store, &mut env, legacy::snapshot0::fd_seek),
        //     "fd_sync" => Func::new_env(&store, &mut env, fd_sync),
        //     "fd_tell" => Func::new_env(&store, &mut env, fd_tell),
        //     "fd_write" => Func::new_env(&store, &mut env, fd_write),
        //     "path_create_directory" => Func::new_env(&store, &mut env, path_create_directory),
        //     "path_filestat_get" => Func::new_env(&store, &mut env, legacy::snapshot0::path_filestat_get),
        //     "path_filestat_set_times" => Func::new_env(&store, &mut env, path_filestat_set_times),
        //     "path_link" => Func::new_env(&store, &mut env, path_link),
        //     "path_open" => Func::new_env(&store, &mut env, path_open),
        //     "path_readlink" => Func::new_env(&store, &mut env, path_readlink),
        //     "path_remove_directory" => Func::new_env(&store, &mut env, path_remove_directory),
        //     "path_rename" => Func::new_env(&store, &mut env, path_rename),
        //     "path_symlink" => Func::new_env(&store, &mut env, path_symlink),
        //     "path_unlink_file" => Func::new_env(&store, &mut env, path_unlink_file),
        //     "poll_oneoff" => Func::new_env(&store, &mut env, legacy::snapshot0::poll_oneoff),
        //     "proc_exit" => Func::new_env(&store, &mut env, proc_exit),
        //     "proc_raise" => Func::new_env(&store, &mut env, proc_raise),
        //     "random_get" => Func::new_env(&store, &mut env, random_get),
        //     "sched_yield" => Func::new_env(&store, &mut env, sched_yield),
        //     "sock_recv" => Func::new_env(&store, &mut env, sock_recv),
        //     "sock_send" => Func::new_env(&store, &mut env, sock_send),
        //     "sock_shutdown" => Func::new_env(&store, &mut env, sock_shutdown),
        // },
    }
}

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_snapshot1(store: &Store, env: &mut WasiEnv) -> ImportObject {
    imports! {
        // "wasi_snapshot_preview1" => {
        //     "args_get" => Func::new_env(&store, &mut env, args_get),
        //     "args_sizes_get" => Func::new_env(&store, &mut env, args_sizes_get),
        //     "clock_res_get" => Func::new_env(&store, &mut env, clock_res_get),
        //     "clock_time_get" => Func::new_env(&store, &mut env, clock_time_get),
        //     "environ_get" => Func::new_env(&store, &mut env, environ_get),
        //     "environ_sizes_get" => Func::new_env(&store, &mut env, environ_sizes_get),
        //     "fd_advise" => Func::new_env(&store, &mut env, fd_advise),
        //     "fd_allocate" => Func::new_env(&store, &mut env, fd_allocate),
        //     "fd_close" => Func::new_env(&store, &mut env, fd_close),
        //     "fd_datasync" => Func::new_env(&store, &mut env, fd_datasync),
        //     "fd_fdstat_get" => Func::new_env(&store, &mut env, fd_fdstat_get),
        //     "fd_fdstat_set_flags" => Func::new_env(&store, &mut env, fd_fdstat_set_flags),
        //     "fd_fdstat_set_rights" => Func::new_env(&store, &mut env, fd_fdstat_set_rights),
        //     "fd_filestat_get" => Func::new_env(&store, &mut env, fd_filestat_get),
        //     "fd_filestat_set_size" => Func::new_env(&store, &mut env, fd_filestat_set_size),
        //     "fd_filestat_set_times" => Func::new_env(&store, &mut env, fd_filestat_set_times),
        //     "fd_pread" => Func::new_env(&store, &mut env, fd_pread),
        //     "fd_prestat_get" => Func::new_env(&store, &mut env, fd_prestat_get),
        //     "fd_prestat_dir_name" => Func::new_env(&store, &mut env, fd_prestat_dir_name),
        //     "fd_pwrite" => Func::new_env(&store, &mut env, fd_pwrite),
        //     "fd_read" => Func::new_env(&store, &mut env, fd_read),
        //     "fd_readdir" => Func::new_env(&store, &mut env, fd_readdir),
        //     "fd_renumber" => Func::new_env(&store, &mut env, fd_renumber),
        //     "fd_seek" => Func::new_env(&store, &mut env, fd_seek),
        //     "fd_sync" => Func::new_env(&store, &mut env, fd_sync),
        //     "fd_tell" => Func::new_env(&store, &mut env, fd_tell),
        //     "fd_write" => Func::new_env(&store, &mut env, fd_write),
        //     "path_create_directory" => Func::new_env(&store, &mut env, path_create_directory),
        //     "path_filestat_get" => Func::new_env(&store, &mut env, path_filestat_get),
        //     "path_filestat_set_times" => Func::new_env(&store, &mut env, path_filestat_set_times),
        //     "path_link" => Func::new_env(&store, &mut env, path_link),
        //     "path_open" => Func::new_env(&store, &mut env, path_open),
        //     "path_readlink" => Func::new_env(&store, &mut env, path_readlink),
        //     "path_remove_directory" => Func::new_env(&store, &mut env, path_remove_directory),
        //     "path_rename" => Func::new_env(&store, &mut env, path_rename),
        //     "path_symlink" => Func::new_env(&store, &mut env, path_symlink),
        //     "path_unlink_file" => Func::new_env(&store, &mut env, path_unlink_file),
        //     "poll_oneoff" => Func::new_env(&store, &mut env, poll_oneoff),
        //     "proc_exit" => Func::new_env(&store, &mut env, proc_exit),
        //     "proc_raise" => Func::new_env(&store, &mut env, proc_raise),
        //     "random_get" => Func::new_env(&store, &mut env, random_get),
        //     "sched_yield" => Func::new_env(&store, &mut env, sched_yield),
        //     "sock_recv" => Func::new_env(&store, &mut env, sock_recv),
        //     "sock_send" => Func::new_env(&store, &mut env, sock_send),
        //     "sock_shutdown" => Func::new_env(&store, &mut env, sock_shutdown),
        // }
    }
}
