#![deny(
    dead_code,
    nonstandard_style,
    unused_imports,
    unused_mut,
    unused_variables,
    unused_unsafe,
    unreachable_patterns
)]
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

pub use self::utils::{get_wasi_version, is_wasi_module, WasiVersion};

use wasmer_runtime_core::{func, import::ImportObject, imports};

/// This is returned in the Box<dyn Any> RuntimeError::Error variant.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
pub struct ExitCode {
    pub code: syscalls::types::__wasi_exitcode_t,
}

/// Creates a Wasi [`ImportObject`] with [`WasiState`] with the latest snapshot
/// of WASI.
pub fn generate_import_object(
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopened_files: Vec<PathBuf>,
    mapped_dirs: Vec<(String, PathBuf)>,
) -> ImportObject {
    let state_gen = move || {
        // TODO: look into removing all these unnecessary clones
        fn state_destructor(data: *mut c_void) {
            unsafe {
                drop(Box::from_raw(data as *mut WasiState));
            }
        }
        let preopened_files = preopened_files.clone();
        let mapped_dirs = mapped_dirs.clone();

        let state = Box::new(WasiState {
            fs: WasiFs::new(&preopened_files, &mapped_dirs).expect("Could not create WASI FS"),
            args: args.clone(),
            envs: envs.clone(),
        });

        (
            Box::into_raw(state) as *mut c_void,
            state_destructor as fn(*mut c_void),
        )
    };

    generate_import_object_snapshot1_inner(state_gen)
}

/// Create an [`ImportObject`] with an existing [`WasiState`]. [`WasiState`]
/// can be constructed from a [`WasiStateBuilder`](state::WasiStateBuilder).
pub fn generate_import_object_from_state(
    wasi_state: WasiState,
    version: WasiVersion,
) -> ImportObject {
    // HACK(mark): this is really quite nasty and inefficient, a proper fix will
    //             require substantial changes to the internals of the WasiFS
    // copy WasiState by serializing and deserializing
    let wasi_state_bytes = wasi_state.freeze().unwrap();
    let state_gen = move || {
        fn state_destructor(data: *mut c_void) {
            unsafe {
                drop(Box::from_raw(data as *mut WasiState));
            }
        }

        let wasi_state = Box::new(WasiState::unfreeze(&wasi_state_bytes).unwrap());

        (
            Box::into_raw(wasi_state) as *mut c_void,
            state_destructor as fn(*mut c_void),
        )
    };
    match version {
        WasiVersion::Snapshot0 => generate_import_object_snapshot0_inner(state_gen),
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object_snapshot1_inner(state_gen)
        }
    }
}

/// Creates a Wasi [`ImportObject`] with [`WasiState`] for the given [`WasiVersion`].
pub fn generate_import_object_for_version(
    version: WasiVersion,
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopened_files: Vec<PathBuf>,
    mapped_dirs: Vec<(String, PathBuf)>,
) -> ImportObject {
    match version {
        WasiVersion::Snapshot0 => {
            generate_import_object_snapshot0(args, envs, preopened_files, mapped_dirs)
        }
        WasiVersion::Snapshot1 | WasiVersion::Latest => {
            generate_import_object(args, envs, preopened_files, mapped_dirs)
        }
    }
}

/// Creates a legacy Wasi [`ImportObject`] with [`WasiState`].
fn generate_import_object_snapshot0(
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopened_files: Vec<PathBuf>,
    mapped_dirs: Vec<(String, PathBuf)>,
) -> ImportObject {
    let state_gen = move || {
        // TODO: look into removing all these unnecessary clones
        fn state_destructor(data: *mut c_void) {
            unsafe {
                drop(Box::from_raw(data as *mut WasiState));
            }
        }
        let preopened_files = preopened_files.clone();
        let mapped_dirs = mapped_dirs.clone();
        //let wasi_builder = create_wasi_instance();

        let state = Box::new(WasiState {
            fs: WasiFs::new(&preopened_files, &mapped_dirs).expect("Could not create WASI FS"),
            args: args.clone(),
            envs: envs.clone(),
        });

        (
            Box::into_raw(state) as *mut c_void,
            state_destructor as fn(*mut c_void),
        )
    };
    generate_import_object_snapshot0_inner(state_gen)
}

/// Combines a state generating function with the import list for legacy WASI
fn generate_import_object_snapshot0_inner<F>(state_gen: F) -> ImportObject
where
    F: Fn() -> (*mut c_void, fn(*mut c_void)) + Send + Sync + 'static,
{
    imports! {
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
            "fd_filestat_get" => func!(legacy::snapshot0::fd_filestat_get),
            "fd_filestat_set_size" => func!(fd_filestat_set_size),
            "fd_filestat_set_times" => func!(fd_filestat_set_times),
            "fd_pread" => func!(fd_pread),
            "fd_prestat_get" => func!(fd_prestat_get),
            "fd_prestat_dir_name" => func!(fd_prestat_dir_name),
            "fd_pwrite" => func!(fd_pwrite),
            "fd_read" => func!(fd_read),
            "fd_readdir" => func!(fd_readdir),
            "fd_renumber" => func!(fd_renumber),
            "fd_seek" => func!(legacy::snapshot0::fd_seek),
            "fd_sync" => func!(fd_sync),
            "fd_tell" => func!(fd_tell),
            "fd_write" => func!(fd_write),
            "path_create_directory" => func!(path_create_directory),
            "path_filestat_get" => func!(legacy::snapshot0::path_filestat_get),
            "path_filestat_set_times" => func!(path_filestat_set_times),
            "path_link" => func!(path_link),
            "path_open" => func!(path_open),
            "path_readlink" => func!(path_readlink),
            "path_remove_directory" => func!(path_remove_directory),
            "path_rename" => func!(path_rename),
            "path_symlink" => func!(path_symlink),
            "path_unlink_file" => func!(path_unlink_file),
            "poll_oneoff" => func!(legacy::snapshot0::poll_oneoff),
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

/// Combines a state generating function with the import list for snapshot 1
fn generate_import_object_snapshot1_inner<F>(state_gen: F) -> ImportObject
where
    F: Fn() -> (*mut c_void, fn(*mut c_void)) + Send + Sync + 'static,
{
    imports! {
            state_gen,
            "wasi_snapshot_preview1" => {
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
