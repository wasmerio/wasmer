#![deny(dead_code)]
use crate::{WasiEnv, WasiError, WasiState, WasiThread};
use wasmer::{Memory, Memory32, MemorySize, WasmPtr, WasmSlice};
use wasmer_wasi_types::*;

type MemoryType = Memory32;
type MemoryOffset = u32;

pub(crate) fn args_get(
    env: &WasiEnv,
    argv: WasmPtr<WasmPtr<u8, MemoryType>, MemoryType>,
    argv_buf: WasmPtr<u8, MemoryType>,
) -> __wasi_errno_t {
    super::args_get::<MemoryType>(env, argv, argv_buf)
}

pub(crate) fn args_sizes_get(
    env: &WasiEnv,
    argc: WasmPtr<MemoryOffset, MemoryType>,
    argv_buf_size: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::args_sizes_get::<MemoryType>(env, argc, argv_buf_size)
}

pub(crate) fn clock_res_get(
    env: &WasiEnv,
    clock_id: __wasi_clockid_t,
    resolution: WasmPtr<__wasi_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::clock_res_get::<MemoryType>(env, clock_id, resolution)
}

pub(crate) fn clock_time_get(
    env: &WasiEnv,
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::clock_time_get::<MemoryType>(env, clock_id, precision, time)
}

pub(crate) fn environ_get(
    env: &WasiEnv,
    environ: WasmPtr<WasmPtr<u8, MemoryType>, MemoryType>,
    environ_buf: WasmPtr<u8, MemoryType>,
) -> __wasi_errno_t {
    super::environ_get::<MemoryType>(env, environ, environ_buf)
}

pub(crate) fn environ_sizes_get(
    env: &WasiEnv,
    environ_count: WasmPtr<MemoryOffset, MemoryType>,
    environ_buf_size: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::environ_sizes_get::<MemoryType>(env, environ_count, environ_buf_size)
}

pub(crate) fn fd_advise(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
    advice: __wasi_advice_t,
) -> __wasi_errno_t {
    super::fd_advise(env, fd, offset, len, advice)
}

pub(crate) fn fd_allocate(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
) -> __wasi_errno_t {
    super::fd_allocate(env, fd, offset, len)
}

pub(crate) fn fd_close(env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_close(env, fd)
}

pub(crate) fn fd_datasync(env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_datasync(env, fd)
}

pub(crate) fn fd_fdstat_get(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    buf_ptr: WasmPtr<__wasi_fdstat_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_fdstat_get::<MemoryType>(env, fd, buf_ptr)
}

pub(crate) fn fd_fdstat_set_flags(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    flags: __wasi_fdflags_t,
) -> __wasi_errno_t {
    super::fd_fdstat_set_flags(env, fd, flags)
}

pub(crate) fn fd_fdstat_set_rights(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
) -> __wasi_errno_t {
    super::fd_fdstat_set_rights(env, fd, fs_rights_base, fs_rights_inheriting)
}

pub(crate) fn fd_filestat_get(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_filestat_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_filestat_get::<MemoryType>(env, fd, buf)
}

pub(crate) fn fd_filestat_set_size(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    st_size: __wasi_filesize_t,
) -> __wasi_errno_t {
    super::fd_filestat_set_size(env, fd, st_size)
}

pub(crate) fn fd_filestat_set_times(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    super::fd_filestat_set_times(env, fd, st_atim, st_mtim, fst_flags)
}

pub(crate) fn fd_pread(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    offset: __wasi_filesize_t,
    nread: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_pread::<MemoryType>(env, fd, iovs, iovs_len, offset, nread)
}

pub(crate) fn fd_prestat_get(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_prestat_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_prestat_get::<MemoryType>(env, fd, buf)
}

pub(crate) fn fd_prestat_dir_name(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::fd_prestat_dir_name::<MemoryType>(env, fd, path, path_len)
}

pub(crate) fn fd_pwrite(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    offset: __wasi_filesize_t,
    nwritten: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_pwrite::<MemoryType>(env, fd, iovs, iovs_len, offset, nwritten)
}

pub(crate) fn fd_read(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    nread: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_read::<MemoryType>(env, fd, iovs, iovs_len, nread)
}

pub(crate) fn fd_readdir(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    cookie: __wasi_dircookie_t,
    bufused: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::fd_readdir::<MemoryType>(env, fd, buf, buf_len, cookie, bufused)
}

pub(crate) fn fd_renumber(env: &WasiEnv, from: __wasi_fd_t, to: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_renumber(env, from, to)
}

pub(crate) fn fd_seek(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    offset: __wasi_filedelta_t,
    whence: __wasi_whence_t,
    newoffset: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_seek::<MemoryType>(env, fd, offset, whence, newoffset)
}

pub(crate) fn fd_sync(env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_sync(env, fd)
}

pub(crate) fn fd_tell(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    offset: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_tell::<MemoryType>(env, fd, offset)
}

pub(crate) fn fd_write(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    nwritten: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_write::<MemoryType>(env, fd, iovs, iovs_len, nwritten)
}

pub(crate) fn path_create_directory(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_create_directory::<MemoryType>(env, fd, path, path_len)
}

pub(crate) fn path_filestat_get(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    buf: WasmPtr<__wasi_filestat_t, MemoryType>,
) -> __wasi_errno_t {
    super::path_filestat_get::<MemoryType>(env, fd, flags, path, path_len, buf)
}

pub(crate) fn path_filestat_set_times(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    super::path_filestat_set_times::<MemoryType>(
        env, fd, flags, path, path_len, st_atim, st_mtim, fst_flags,
    )
}

pub(crate) fn path_link(
    env: &WasiEnv,
    old_fd: __wasi_fd_t,
    old_flags: __wasi_lookupflags_t,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_link::<MemoryType>(
        env,
        old_fd,
        old_flags,
        old_path,
        old_path_len,
        new_fd,
        new_path,
        new_path_len,
    )
}

pub(crate) fn path_open(
    env: &WasiEnv,
    dirfd: __wasi_fd_t,
    dirflags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    o_flags: __wasi_oflags_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
    fs_flags: __wasi_fdflags_t,
    fd: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::path_open::<MemoryType>(
        env,
        dirfd,
        dirflags,
        path,
        path_len,
        o_flags,
        fs_rights_base,
        fs_rights_inheriting,
        fs_flags,
        fd,
    )
}

pub(crate) fn path_readlink(
    env: &WasiEnv,
    dir_fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    buf_used: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::path_readlink::<MemoryType>(env, dir_fd, path, path_len, buf, buf_len, buf_used)
}

pub(crate) fn path_remove_directory(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_remove_directory::<MemoryType>(env, fd, path, path_len)
}

pub(crate) fn path_rename(
    env: &WasiEnv,
    old_fd: __wasi_fd_t,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_rename::<MemoryType>(
        env,
        old_fd,
        old_path,
        old_path_len,
        new_fd,
        new_path,
        new_path_len,
    )
}

pub(crate) fn path_symlink(
    env: &WasiEnv,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_symlink::<MemoryType>(env, old_path, old_path_len, fd, new_path, new_path_len)
}

pub(crate) fn path_unlink_file(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_unlink_file::<MemoryType>(env, fd, path, path_len)
}

pub(crate) fn poll_oneoff(
    env: &WasiEnv,
    in_: WasmPtr<__wasi_subscription_t, MemoryType>,
    out_: WasmPtr<__wasi_event_t, MemoryType>,
    nsubscriptions: MemoryOffset,
    nevents: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::poll_oneoff::<MemoryType>(env, in_, out_, nsubscriptions, nevents)
}

pub(crate) fn proc_exit(env: &WasiEnv, code: __wasi_exitcode_t) -> Result<(), WasiError> {
    super::proc_exit(env, code)
}

pub(crate) fn proc_raise(env: &WasiEnv, sig: __wasi_signal_t) -> __wasi_errno_t {
    super::proc_raise(env, sig)
}

pub(crate) fn random_get(
    env: &WasiEnv,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
) -> __wasi_errno_t {
    super::random_get::<MemoryType>(env, buf, buf_len)
}

pub(crate) fn sched_yield(env: &WasiEnv) -> Result<__wasi_errno_t, WasiError> {
    super::sched_yield(env)
}

pub(crate) fn sock_recv(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    ri_data: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    ri_data_len: MemoryOffset,
    ri_flags: __wasi_riflags_t,
    ro_data_len: WasmPtr<MemoryOffset, MemoryType>,
    ro_flags: WasmPtr<__wasi_roflags_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_recv::<MemoryType>(
        env,
        sock,
        ri_data,
        ri_data_len,
        ri_flags,
        ro_data_len,
        ro_flags,
    )
}

pub(crate) fn sock_send(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    si_data_len: MemoryOffset,
    si_flags: __wasi_siflags_t,
    ret_data_len: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_send::<MemoryType>(env, sock, si_data, si_data_len, si_flags, ret_data_len)
}

pub(crate) fn sock_shutdown(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    how: __wasi_sdflags_t,
) -> __wasi_errno_t {
    super::sock_shutdown(env, sock, how)
}
