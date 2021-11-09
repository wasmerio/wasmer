use super::types::*;

use crate::{
    ptr::{Array, WasmPtr},
    state::{
        self, fs_error_into_wasi_err, iterate_poll_events,
        virtual_file_type_to_wasi_file_type, Fd, Inode, InodeVal, Kind,
        PollEventBuilder, WasiState, MAX_SYMLINKS,
    },
    WasiEnv, WasiError,
};
pub use wasmer_vfs::{FsError, VirtualFile};
pub use crate::state::{
    PollEvent, PollEventSet,
};

pub trait WasiProxy
where Self: Send + Sync + std::fmt::Debug
{
    fn args_get(&self, env: &WasiEnv, argv: WasmPtr<WasmPtr<u8, Array>, Array>, argv_buf: WasmPtr<u8, Array>) -> __wasi_errno_t {
        super::native::args_get(env, argv, argv_buf)
    }
    fn args_sizes_get(&self, env: &WasiEnv, argc: WasmPtr<u32>, argv_buf_size: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::args_sizes_get(env, argc, argv_buf_size)
    }
    fn clock_res_get(&self, env: &WasiEnv, clock_id: __wasi_clockid_t) -> Result<__wasi_timestamp_t, __wasi_errno_t> {
        super::native::clock_res_get(env, clock_id)
    }
    fn clock_time_get(&self, env: &WasiEnv, clock_id: __wasi_clockid_t, precision: __wasi_timestamp_t) -> Result<__wasi_timestamp_t, __wasi_errno_t> {
        super::native::clock_time_get(env, clock_id, precision)
    }
    fn environ_get(&self, env: &WasiEnv, environ: WasmPtr<WasmPtr<u8, Array>, Array>, environ_buf: WasmPtr<u8, Array>) -> __wasi_errno_t {
        super::native::environ_get(env, environ, environ_buf)
    }
    fn environ_sizes_get(&self, env: &WasiEnv, environ_count: WasmPtr<u32>, environ_buf_size: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::environ_sizes_get(env, environ_count, environ_buf_size)
    }
    fn fd_advise(&self, env: &WasiEnv, fd: __wasi_fd_t, offset: __wasi_filesize_t, len: __wasi_filesize_t, advice: __wasi_advice_t) -> __wasi_errno_t {
        super::native::fd_advise(env, fd, offset, len, advice)
    }
    fn fd_allocate(&self, env: &WasiEnv, fd: __wasi_fd_t, offset: __wasi_filesize_t, len: __wasi_filesize_t) -> __wasi_errno_t {
        super::native::fd_allocate(env, fd, offset, len)
    }
    fn fd_close(&self, env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
        super::native::fd_close(env, fd)
    }
    fn fd_datasync(&self, env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
        super::native::fd_datasync(env, fd)
    }
    fn fd_fdstat_get(&self, env: &WasiEnv, fd: __wasi_fd_t, buf_ptr: WasmPtr<__wasi_fdstat_t>) -> __wasi_errno_t {
        super::native::fd_fdstat_get(env, fd, buf_ptr)
    }
    fn fd_fdstat_set_flags(&self, env: &WasiEnv, fd: __wasi_fd_t, flags: __wasi_fdflags_t) -> __wasi_errno_t {
        super::native::fd_fdstat_set_flags(env, fd, flags)
    }
    fn fd_fdstat_set_rights(&self, env: &WasiEnv, fd: __wasi_fd_t, fs_rights_base: __wasi_rights_t, fs_rights_inheriting: __wasi_rights_t) -> __wasi_errno_t {
        super::native::fd_fdstat_set_rights(env, fd, fs_rights_base, fs_rights_inheriting)
    }
    fn fd_filestat_get(&self, env: &WasiEnv, fd: __wasi_fd_t, buf: WasmPtr<__wasi_filestat_t>) -> __wasi_errno_t {
        super::native::fd_filestat_get(env, fd, buf)
    }
    fn fd_filestat_set_size(&self, env: &WasiEnv, fd: __wasi_fd_t, st_size: __wasi_filesize_t) -> __wasi_errno_t {
        super::native::fd_filestat_set_size(env, fd, st_size)
    }
    fn fd_filestat_set_times(&self, env: &WasiEnv, fd: __wasi_fd_t, st_atim: __wasi_timestamp_t, st_mtim: __wasi_timestamp_t, fst_flags: __wasi_fstflags_t) -> __wasi_errno_t {
        super::native::fd_filestat_set_times(env, fd, st_atim, st_mtim, fst_flags)
    }
    fn fd_pread(&self, env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_iovec_t, Array>, iovs_len: u32, offset: __wasi_filesize_t, nread: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::fd_pread(env, fd, iovs, iovs_len, offset, nread)
    }
    fn fd_prestat_get(&self, env: &WasiEnv, fd: __wasi_fd_t, buf: WasmPtr<__wasi_prestat_t>) -> __wasi_errno_t {
        super::native::fd_prestat_get(env, fd, buf)
    }
    fn fd_prestat_dir_name(&self, env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
        super::native::fd_prestat_dir_name(env, fd, path, path_len)
    }
    fn fd_pwrite(&self, env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_ciovec_t, Array>, iovs_len: u32, offset: __wasi_filesize_t, nwritten: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::fd_pwrite(env, fd, iovs, iovs_len, offset, nwritten)
    }
    fn fd_read(&self, env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_iovec_t, Array>, iovs_len: u32, nread: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::fd_read(env, fd, iovs, iovs_len, nread)
    }
    fn fd_readdir(&self, env: &WasiEnv, fd: __wasi_fd_t, buf: WasmPtr<u8, Array>, buf_len: u32, cookie: __wasi_dircookie_t, bufused: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::fd_readdir(env, fd, buf, buf_len, cookie, bufused)
    }
    fn fd_renumber(&self, env: &WasiEnv, from: __wasi_fd_t, to: __wasi_fd_t) -> __wasi_errno_t {
        super::native::fd_renumber(env, from, to)
    }
    fn fd_seek(&self, env: &WasiEnv, fd: __wasi_fd_t, offset: __wasi_filedelta_t, whence: __wasi_whence_t, newoffset: WasmPtr<__wasi_filesize_t>) -> __wasi_errno_t {
        super::native::fd_seek(env, fd, offset, whence, newoffset)
    }
    fn fd_sync(&self, env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
        super::native::fd_sync(env, fd)
    }
    fn fd_tell(&self, env: &WasiEnv, fd: __wasi_fd_t, offset: WasmPtr<__wasi_filesize_t>) -> __wasi_errno_t {
        super::native::fd_tell(env, fd, offset)
    }
    fn fd_write(&self, env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_ciovec_t, Array>, iovs_len: u32, nwritten: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::fd_write(env, fd, iovs, iovs_len, nwritten)
    }
    fn path_create_directory(&self, env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
        super::native::path_create_directory(env, fd, path, path_len)
    }
    fn path_filestat_get(&self, env: &WasiEnv, fd: __wasi_fd_t, flags: __wasi_lookupflags_t, path: WasmPtr<u8, Array>, path_len: u32, buf: WasmPtr<__wasi_filestat_t>) -> __wasi_errno_t {
        super::native::path_filestat_get(env, fd, flags, path, path_len, buf)
    }
    fn path_filestat_set_times(&self, env: &WasiEnv, fd: __wasi_fd_t, flags: __wasi_lookupflags_t, path: WasmPtr<u8, Array>, path_len: u32, st_atim: __wasi_timestamp_t, st_mtim: __wasi_timestamp_t, fst_flags: __wasi_fstflags_t) -> __wasi_errno_t {
        super::native::path_filestat_set_times(env, fd, flags, path, path_len, st_atim, st_mtim, fst_flags)
    }
    fn path_link(&self, env: &WasiEnv, old_fd: __wasi_fd_t, old_flags: __wasi_lookupflags_t, old_path: WasmPtr<u8, Array>, old_path_len: u32, new_fd: __wasi_fd_t, new_path: WasmPtr<u8, Array>, new_path_len: u32) -> __wasi_errno_t {
        super::native::path_link(env, old_fd, old_flags, old_path, old_path_len, new_fd, new_path, new_path_len)
    }
    fn path_open(&self, env: &WasiEnv, dirfd: __wasi_fd_t, dirflags: __wasi_lookupflags_t, path: WasmPtr<u8, Array>, path_len: u32, o_flags: __wasi_oflags_t, fs_rights_base: __wasi_rights_t, fs_rights_inheriting: __wasi_rights_t, fs_flags: __wasi_fdflags_t, fd: WasmPtr<__wasi_fd_t>) -> __wasi_errno_t {
        super::native::path_open(env, dirfd, dirflags, path, path_len, o_flags, fs_rights_base, fs_rights_inheriting, fs_flags, fd)
    }
    fn path_readlink(&self, env: &WasiEnv, dir_fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32, buf: WasmPtr<u8, Array>, buf_len: u32, buf_used: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::path_readlink(env, dir_fd, path, path_len, buf, buf_len, buf_used)
    }
    fn path_remove_directory(&self, env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
        super::native::path_remove_directory(env, fd, path, path_len)
    }
    fn path_rename(&self, env: &WasiEnv, old_fd: __wasi_fd_t, old_path: WasmPtr<u8, Array>, old_path_len: u32, new_fd: __wasi_fd_t, new_path: WasmPtr<u8, Array>, new_path_len: u32) -> __wasi_errno_t {
        super::native::path_rename(env, old_fd, old_path, old_path_len, new_fd, new_path, new_path_len)
    }
    fn path_symlink(&self, env: &WasiEnv, old_path: WasmPtr<u8, Array>, old_path_len: u32, fd: __wasi_fd_t, new_path: WasmPtr<u8, Array>, new_path_len: u32) -> __wasi_errno_t {
        super::native::path_symlink(env, old_path, old_path_len, fd, new_path, new_path_len)
    }
    fn path_unlink_file(&self, env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
        super::native::path_unlink_file(env, fd, path, path_len)
    }
    fn poll_oneoff(&self, env: &WasiEnv, in_: WasmPtr<__wasi_subscription_t, Array>, out_: WasmPtr<__wasi_event_t, Array>, nsubscriptions: u32, nevents: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::poll_oneoff(env, in_, out_, nsubscriptions, nevents)
    }
    fn poll(&self, env: &WasiEnv, files: &[&dyn VirtualFile], events: &[PollEventSet], seen_events: &mut [PollEventSet]) -> Result<u32, FsError> {
        super::native::poll(env, files, events, seen_events)
    }
    fn proc_exit(&self, env: &WasiEnv, code: __wasi_exitcode_t) {
        super::native::proc_exit(env, code)
    }
    fn proc_raise(&self, env: &WasiEnv, sig: __wasi_signal_t) -> __wasi_errno_t {
        super::native::proc_raise(env, sig)
    }
    fn random_get(&self, env: &WasiEnv, buf: u32, buf_len: u32) -> __wasi_errno_t {
        super::native::random_get(env, buf, buf_len)
    }
    fn sched_yield(&self, env: &WasiEnv) -> __wasi_errno_t {
        super::native::sched_yield(env)
    }
    fn sock_recv(&self, env: &WasiEnv, sock: __wasi_fd_t, ri_data: WasmPtr<__wasi_iovec_t, Array>, ri_data_len: u32, ri_flags: __wasi_riflags_t, ro_datalen: WasmPtr<u32>, ro_flags: WasmPtr<__wasi_roflags_t>) -> __wasi_errno_t {
        super::native::sock_recv(env, sock, ri_data, ri_data_len, ri_flags, ro_datalen, ro_flags)
    }
    fn sock_send(&self, env: &WasiEnv, sock: __wasi_fd_t, si_data: WasmPtr<__wasi_ciovec_t, Array>, si_data_len: u32, si_flags: __wasi_siflags_t, so_datalen: WasmPtr<u32>) -> __wasi_errno_t {
        super::native::sock_send(env, sock, si_data, si_data_len, si_flags, so_datalen)
    }
    fn sock_shutdown(&self, env: &WasiEnv, sock: __wasi_fd_t, how: __wasi_sdflags_t) -> __wasi_errno_t {
        super::native::sock_shutdown(env, sock, how)
    }
}

#[derive(Debug, Default)]
pub struct DefaultWasiProxy { }
impl WasiProxy for DefaultWasiProxy { }