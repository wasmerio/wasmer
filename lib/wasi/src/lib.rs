mod state;
mod syscalls;
use self::state::WasiState;
use self::syscalls::*;

use std::ffi::c_void;

use wasmer_runtime_core::{func, import::ImportObject, imports};

pub fn generate_import_object(args: Vec<u8>, envs: Vec<u8>) -> ImportObject {
    let state_gen = move || {
        fn state_dtor(data: *mut c_void) {
            unsafe {
                drop(Box::from_raw(data as *mut WasiState));
            }
        }

        let state = Box::new(WasiState {
            args: &args[..],
            envs: &envs[..],
        });

        (
            Box::leak(state) as *mut WasiState as *mut c_void,
            state_dtor as fn(*mut c_void),
        )
    };
    imports! {
        // This generates the wasi state.
        state_gen,
        "wasi_unstable" => {
            "__wasi_args_get" => func!(__wasi_args_get),
            "__wasi_args_sizes_get" => func!(__wasi_args_sizes_get),
            "__wasi_clock_res_get" => func!(__wasi_clock_res_get),
            "__wasi_clock_time_get" => func!(__wasi_clock_time_get),
            "__wasi_environ_get" => func!(__wasi_environ_get),
            "__wasi_environ_sizes_get" => func!(__wasi_environ_sizes_get),
            "__wasi_fd_advise" => func!(__wasi_fd_advise),
            "__wasi_fd_allocate" => func!(__wasi_fd_allocate),
            "__wasi_fd_close" => func!(__wasi_fd_close),
            "__wasi_fd_datasync" => func!(__wasi_fd_datasync),
            "__wasi_fd_fdstat_get" => func!(__wasi_fd_fdstat_get),
            "__wasi_fd_fdstat_set_flags" => func!(__wasi_fd_fdstat_set_flags),
            "__wasi_fd_fdstat_set_rights" => func!(__wasi_fd_fdstat_set_rights),
            "__wasi_fd_filestat_get" => func!(__wasi_fd_filestat_get),
            "__wasi_fd_filestat_set_size" => func!(__wasi_fd_filestat_set_size),
            "__wasi_fd_filestat_set_times" => func!(__wasi_fd_filestat_set_times),
            "__wasi_fd_pread" => func!(__wasi_fd_pread),
            "__wasi_fd_prestat_get" => func!(__wasi_fd_prestat_get),
            "__wasi_fd_prestat_dir_name" => func!(__wasi_fd_prestat_dir_name),
            "__wasi_fd_pwrite" => func!(__wasi_fd_pwrite),
            "__wasi_fd_read" => func!(__wasi_fd_read),
            "__wasi_fd_readdir" => func!(__wasi_fd_readdir),
            "__wasi_fd_renumber" => func!(__wasi_fd_renumber),
            "__wasi_fd_seek" => func!(__wasi_fd_seek),
            "__wasi_fd_sync" => func!(__wasi_fd_sync),
            "__wasi_fd_tell" => func!(__wasi_fd_tell),
            "__wasi_fd_write" => func!(__wasi_fd_write),
            "__wasi_path_create_directory" => func!(__wasi_path_create_directory),
            "__wasi_path_filestat_get" => func!(__wasi_path_filestat_get),
            "__wasi_path_filestat_set_times" => func!(__wasi_path_filestat_set_times),
            "__wasi_path_link" => func!(__wasi_path_link),
            "__wasi_path_open" => func!(__wasi_path_open),
            "__wasi_path_readlink" => func!(__wasi_path_readlink),
            "__wasi_path_remove_directory" => func!(__wasi_path_remove_directory),
            "__wasi_path_rename" => func!(__wasi_path_rename),
            "__wasi_path_symlink" => func!(__wasi_path_symlink),
            "__wasi_path_unlink_file" => func!(__wasi_path_unlink_file),
            "__wasi_poll_oneoff" => func!(__wasi_poll_oneoff),
            "__wasi_proc_exit" => func!(__wasi_proc_exit),
            "__wasi_proc_raise" => func!(__wasi_proc_raise),
            "__wasi_random_get" => func!(__wasi_random_get),
            "__wasi_sched_yield" => func!(__wasi_sched_yield),
            "__wasi_sock_recv" => func!(__wasi_sock_recv),
            "__wasi_sock_send" => func!(__wasi_sock_send),
            "__wasi_sock_shutdown" => func!(__wasi_sock_shutdown),
        },
    }
}
