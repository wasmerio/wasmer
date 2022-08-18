#![deny(dead_code)]
use crate::{WasiEnv, WasiError, WasiState, WasiThread};
use wasmer::{Memory, Memory32, MemorySize, StoreMut, WasmPtr, WasmSlice};
use wasmer_wasi_types::*;

type MemoryType = Memory32;
type MemoryOffset = u32;

pub(crate) fn args_get(
    ctx: FunctionEnvMut<WasiEnv>,
    argv: WasmPtr<WasmPtr<u8, MemoryType>, MemoryType>,
    argv_buf: WasmPtr<u8, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::args_get::<MemoryType>(ctx, argv, argv_buf)
}

pub(crate) fn args_sizes_get(
    ctx: FunctionEnvMut<WasiEnv>,
    argc: WasmPtr<MemoryOffset, MemoryType>,
    argv_buf_size: WasmPtr<MemoryOffset, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::args_sizes_get::<MemoryType>(ctx, argc, argv_buf_size)
}

pub(crate) fn clock_res_get(
    ctx: FunctionEnvMut<WasiEnv>,
    clock_id: wasi_snapshot0::Clockid,
    resolution: WasmPtr<__wasi_timestamp_t, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::clock_res_get::<MemoryType>(ctx, clock_id, resolution)
}

pub(crate) fn clock_time_get(
    ctx: FunctionEnvMut<WasiEnv>,
    clock_id: wasi_snapshot0::Clockid,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::clock_time_get::<MemoryType>(ctx, clock_id, precision, time)
}

pub(crate) fn environ_get(
    ctx: FunctionEnvMut<WasiEnv>,
    environ: WasmPtr<WasmPtr<u8, MemoryType>, MemoryType>,
    environ_buf: WasmPtr<u8, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::environ_get::<MemoryType>(ctx, environ, environ_buf)
}

pub(crate) fn environ_sizes_get(
    ctx: FunctionEnvMut<WasiEnv>,
    environ_count: WasmPtr<MemoryOffset, MemoryType>,
    environ_buf_size: WasmPtr<MemoryOffset, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::environ_sizes_get::<MemoryType>(ctx, environ_count, environ_buf_size)
}

pub(crate) fn fd_advise(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
    advice: __wasi_advice_t,
) -> wasi_snapshot0::Errno {
    super::fd_advise(ctx, fd, offset, len, advice)
}

pub(crate) fn fd_allocate(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
) -> wasi_snapshot0::Errno {
    super::fd_allocate(ctx, fd, offset, len)
}

pub(crate) fn fd_close(ctx: FunctionEnvMut<WasiEnv>, fd: __wasi_fd_t) -> wasi_snapshot0::Errno {
    super::fd_close(ctx, fd)
}

pub(crate) fn fd_datasync(ctx: FunctionEnvMut<WasiEnv>, fd: __wasi_fd_t) -> wasi_snapshot0::Errno {
    super::fd_datasync(ctx, fd)
}

pub(crate) fn fd_fdstat_get(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    buf_ptr: WasmPtr<wasi_snapshot0::Fdstat, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::fd_fdstat_get::<MemoryType>(ctx, fd, buf_ptr)
}

pub(crate) fn fd_fdstat_set_flags(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    flags: wasi_snapshot0::Fdflags,
) -> wasi_snapshot0::Errno {
    super::fd_fdstat_set_flags(ctx, fd, flags)
}

pub(crate) fn fd_fdstat_set_rights(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
) -> wasi_snapshot0::Errno {
    super::fd_fdstat_set_rights(ctx, fd, fs_rights_base, fs_rights_inheriting)
}

pub(crate) fn fd_filestat_get(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_filestat_t, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::fd_filestat_get::<MemoryType>(ctx, fd, buf)
}

pub(crate) fn fd_filestat_set_size(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    st_size: __wasi_filesize_t,
) -> wasi_snapshot0::Errno {
    super::fd_filestat_set_size(ctx, fd, st_size)
}

pub(crate) fn fd_filestat_set_times(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> wasi_snapshot0::Errno {
    super::fd_filestat_set_times(ctx, fd, st_atim, st_mtim, fst_flags)
}

pub(crate) fn fd_pread(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    offset: __wasi_filesize_t,
    nread: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::fd_pread::<MemoryType>(ctx, fd, iovs, iovs_len, offset, nread)
}

pub(crate) fn fd_prestat_get(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_prestat_t, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::fd_prestat_get::<MemoryType>(ctx, fd, buf)
}

pub(crate) fn fd_prestat_dir_name(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::fd_prestat_dir_name::<MemoryType>(ctx, fd, path, path_len)
}

pub(crate) fn fd_pwrite(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    offset: __wasi_filesize_t,
    nwritten: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::fd_pwrite::<MemoryType>(ctx, fd, iovs, iovs_len, offset, nwritten)
}

pub(crate) fn fd_read(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    nread: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::fd_read::<MemoryType>(ctx, fd, iovs, iovs_len, nread)
}

pub(crate) fn fd_readdir(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    cookie: __wasi_dircookie_t,
    bufused: WasmPtr<MemoryOffset, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::fd_readdir::<MemoryType>(ctx, fd, buf, buf_len, cookie, bufused)
}

pub(crate) fn fd_renumber(
    ctx: FunctionEnvMut<WasiEnv>,
    from: __wasi_fd_t,
    to: __wasi_fd_t,
) -> wasi_snapshot0::Errno {
    super::fd_renumber(ctx, from, to)
}

pub(crate) fn fd_seek(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    offset: __wasi_filedelta_t,
    whence: __wasi_whence_t,
    newoffset: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::fd_seek::<MemoryType>(ctx, fd, offset, whence, newoffset)
}

pub(crate) fn fd_sync(ctx: FunctionEnvMut<WasiEnv>, fd: __wasi_fd_t) -> wasi_snapshot0::Errno {
    super::fd_sync(ctx, fd)
}

pub(crate) fn fd_tell(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    offset: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::fd_tell::<MemoryType>(ctx, fd, offset)
}

pub(crate) fn fd_write(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    nwritten: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::fd_write::<MemoryType>(ctx, fd, iovs, iovs_len, nwritten)
}

pub(crate) fn path_create_directory(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::path_create_directory::<MemoryType>(ctx, fd, path, path_len)
}

pub(crate) fn path_filestat_get(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    buf: WasmPtr<__wasi_filestat_t, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::path_filestat_get::<MemoryType>(ctx, fd, flags, path, path_len, buf)
}

pub(crate) fn path_filestat_set_times(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> wasi_snapshot0::Errno {
    super::path_filestat_set_times::<MemoryType>(
        ctx, fd, flags, path, path_len, st_atim, st_mtim, fst_flags,
    )
}

pub(crate) fn path_link(
    ctx: FunctionEnvMut<WasiEnv>,
    old_fd: __wasi_fd_t,
    old_flags: __wasi_lookupflags_t,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::path_link::<MemoryType>(
        ctx,
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
    ctx: FunctionEnvMut<WasiEnv>,
    dirfd: __wasi_fd_t,
    dirflags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    o_flags: __wasi_oflags_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
    fs_flags: wasi_snapshot0::Fdflags,
    fd: WasmPtr<__wasi_fd_t, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::path_open::<MemoryType>(
        ctx,
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
    ctx: FunctionEnvMut<WasiEnv>,
    dir_fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    buf_used: WasmPtr<MemoryOffset, MemoryType>,
) -> wasi_snapshot0::Errno {
    super::path_readlink::<MemoryType>(ctx, dir_fd, path, path_len, buf, buf_len, buf_used)
}

pub(crate) fn path_remove_directory(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::path_remove_directory::<MemoryType>(ctx, fd, path, path_len)
}

pub(crate) fn path_rename(
    ctx: FunctionEnvMut<WasiEnv>,
    old_fd: __wasi_fd_t,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::path_rename::<MemoryType>(
        ctx,
        old_fd,
        old_path,
        old_path_len,
        new_fd,
        new_path,
        new_path_len,
    )
}

pub(crate) fn path_symlink(
    ctx: FunctionEnvMut<WasiEnv>,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::path_symlink::<MemoryType>(ctx, old_path, old_path_len, fd, new_path, new_path_len)
}

pub(crate) fn path_unlink_file(
    ctx: FunctionEnvMut<WasiEnv>,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::path_unlink_file::<MemoryType>(ctx, fd, path, path_len)
}

pub(crate) fn poll_oneoff(
    ctx: FunctionEnvMut<WasiEnv>,
    in_: WasmPtr<__wasi_subscription_t, MemoryType>,
    out_: WasmPtr<__wasi_event_t, MemoryType>,
    nsubscriptions: MemoryOffset,
    nevents: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::poll_oneoff::<MemoryType>(ctx, in_, out_, nsubscriptions, nevents)
}

pub(crate) fn proc_exit(
    ctx: FunctionEnvMut<WasiEnv>,
    code: __wasi_exitcode_t,
) -> Result<(), WasiError> {
    super::proc_exit(ctx, code)
}

pub(crate) fn proc_raise(
    ctx: FunctionEnvMut<WasiEnv>,
    sig: __wasi_signal_t,
) -> wasi_snapshot0::Errno {
    super::proc_raise(ctx, sig)
}

pub(crate) fn random_get(
    ctx: FunctionEnvMut<WasiEnv>,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
) -> wasi_snapshot0::Errno {
    super::random_get::<MemoryType>(ctx, buf, buf_len)
}

pub(crate) fn sched_yield(
    ctx: FunctionEnvMut<WasiEnv>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::sched_yield(ctx)
}

pub(crate) fn sock_recv(
    ctx: FunctionEnvMut<WasiEnv>,
    sock: __wasi_fd_t,
    ri_data: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    ri_data_len: MemoryOffset,
    ri_flags: __wasi_riflags_t,
    ro_data_len: WasmPtr<MemoryOffset, MemoryType>,
    ro_flags: WasmPtr<__wasi_roflags_t, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::sock_recv::<MemoryType>(
        ctx,
        sock,
        ri_data,
        ri_data_len,
        ri_flags,
        ro_data_len,
        ro_flags,
    )
}

pub(crate) fn sock_send(
    ctx: FunctionEnvMut<WasiEnv>,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    si_data_len: MemoryOffset,
    si_flags: __wasi_siflags_t,
    ret_data_len: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<wasi_snapshot0::Errno, WasiError> {
    super::sock_send::<MemoryType>(ctx, sock, si_data, si_data_len, si_flags, ret_data_len)
}

pub(crate) fn sock_shutdown(
    ctx: FunctionEnvMut<WasiEnv>,
    sock: __wasi_fd_t,
    how: __wasi_sdflags_t,
) -> wasi_snapshot0::Errno {
    super::sock_shutdown(ctx, sock, how)
}
