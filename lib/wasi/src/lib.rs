#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
#[cfg(target = "windows")]
extern crate winapi;

#[macro_use]
mod macros;
mod ptr;
pub mod state;
mod syscalls;
mod utils;

use self::state::{WasiFs, WasiState};
pub use self::syscalls::types;
use self::syscalls::*;

use std::ffi::c_void;
use std::path::PathBuf;

pub use self::utils::is_wasi_module;

use wasmer_runtime_core::{func, import::ImportObject, imports};

/// This is returned in the Box<dyn Any> RuntimeError::Error variant.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
pub struct ExitCode {
    pub code: syscalls::types::__wasi_exitcode_t,
}

pub fn generate_import_object(
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopened_files: Vec<String>,
    mapped_dirs: Vec<(String, PathBuf)>,
) -> ImportObject {
    let state_gen = move || {
        fn state_destructor(data: *mut c_void) {
            unsafe {
                drop(Box::from_raw(data as *mut WasiState));
            }
        }

        let state = Box::new(WasiState {
            fs: WasiFs::new(&preopened_files, &mapped_dirs).unwrap(),
            args: &args[..],
            envs: &envs[..],
        });

        (
            Box::leak(state) as *mut WasiState as *mut c_void,
            state_destructor as fn(*mut c_void),
        )
    };
    imports! {
        // This generates the wasi state.
        state_gen,
        "wasi_unstable" => {
            "args_get" => func!(args_get),
            "args_sizes_get" => func!(args_sizes_get),
            "clock_res_get" => func!(clock_res_get),
            "clock_time_get" => func!(clock_time_get),
            "environ_get" => func!(environ_get),
            "environ_sizes_get" => func!(environ_sizes_get),
            "fd_advise" => func!(fd_advise),
            "fd_allocate" => func!(fd_allocate),
            "fd_close" => func!(fd_close),
            "fd_datasync" => func!(fd_datasync),
            "fd_fdstat_get" => func!(fd_fdstat_get),
            "fd_fdstat_set_flags" => func!(fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => func!(fd_fdstat_set_rights),
            "fd_filestat_get" => func!(fd_filestat_get),
            "fd_filestat_set_size" => func!(fd_filestat_set_size),
            "fd_filestat_set_times" => func!(fd_filestat_set_times),
            "fd_pread" => func!(fd_pread),
            "fd_prestat_get" => func!(fd_prestat_get),
            "fd_prestat_dir_name" => func!(fd_prestat_dir_name),
            "fd_pwrite" => func!(fd_pwrite),
            "fd_read" => func!(fd_read),
            "fd_readdir" => func!(fd_readdir),
            "fd_renumber" => func!(fd_renumber),
            "fd_seek" => func!(fd_seek),
            "fd_sync" => func!(fd_sync),
            "fd_tell" => func!(fd_tell),
            "fd_write" => func!(fd_write),
            "path_create_directory" => func!(path_create_directory),
            "path_filestat_get" => func!(path_filestat_get),
            "path_filestat_set_times" => func!(path_filestat_set_times),
            "path_link" => func!(path_link),
            "path_open" => func!(path_open),
            "path_readlink" => func!(path_readlink),
            "path_remove_directory" => func!(path_remove_directory),
            "path_rename" => func!(path_rename),
            "path_symlink" => func!(path_symlink),
            "path_unlink_file" => func!(path_unlink_file),
            "poll_oneoff" => func!(poll_oneoff),
            "proc_exit" => func!(proc_exit),
            "proc_raise" => func!(proc_raise),
            "random_get" => func!(random_get),
            "sched_yield" => func!(sched_yield),
            "sock_recv" => func!(sock_recv),
            "sock_send" => func!(sock_send),
            "sock_shutdown" => func!(sock_shutdown),
        },
    }
}
