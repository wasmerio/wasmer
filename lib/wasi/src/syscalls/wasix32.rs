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

pub(crate) fn fd_dup(
    env: &WasiEnv,
    fd: __wasi_fd_t,
    ret_fd: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_dup::<MemoryType>(env, fd, ret_fd)
}

pub(crate) fn fd_event(
    env: &WasiEnv,
    initial_val: u64,
    flags: __wasi_eventfdflags,
    ret_fd: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_event(env, initial_val, flags, ret_fd)
}

pub(crate) fn fd_pipe(
    env: &WasiEnv,
    ro_fd1: WasmPtr<__wasi_fd_t, MemoryType>,
    ro_fd2: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_pipe::<MemoryType>(env, ro_fd1, ro_fd2)
}

pub(crate) fn tty_get(
    env: &WasiEnv,
    tty_state: WasmPtr<__wasi_tty_t, MemoryType>,
) -> __wasi_errno_t {
    super::tty_get::<MemoryType>(env, tty_state)
}

pub(crate) fn tty_set(
    env: &WasiEnv,
    tty_state: WasmPtr<__wasi_tty_t, MemoryType>,
) -> __wasi_errno_t {
    super::tty_set::<MemoryType>(env, tty_state)
}

pub(crate) fn getcwd(
    env: &WasiEnv,
    path: WasmPtr<u8, MemoryType>,
    path_len: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::getcwd::<MemoryType>(env, path, path_len)
}

pub(crate) fn chdir(
    env: &WasiEnv,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::chdir::<MemoryType>(env, path, path_len)
}

pub(crate) fn thread_spawn(
    env: &WasiEnv,
    method: WasmPtr<u8, MemoryType>,
    method_len: MemoryOffset,
    user_data: u64,
    reactor: __wasi_bool_t,
    ret_tid: WasmPtr<__wasi_tid_t, MemoryType>,
) -> __wasi_errno_t {
    super::thread_spawn::<MemoryType>(env, method, method_len, user_data, reactor, ret_tid)
}

pub(crate) fn thread_sleep(
    env: &WasiEnv,
    duration: __wasi_timestamp_t,
) -> Result<__wasi_errno_t, WasiError> {
    super::thread_sleep(env, duration)
}

pub(crate) fn thread_id(
    env: &WasiEnv,
    ret_tid: WasmPtr<__wasi_tid_t, MemoryType>,
) -> __wasi_errno_t {
    super::thread_id::<MemoryType>(env, ret_tid)
}

pub(crate) fn thread_join(env: &WasiEnv, tid: __wasi_tid_t) -> Result<__wasi_errno_t, WasiError> {
    super::thread_join(env, tid)
}

pub(crate) fn thread_parallelism(
    env: &WasiEnv,
    ret_parallelism: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::thread_parallelism::<MemoryType>(env, ret_parallelism)
}

pub(crate) fn thread_exit(
    env: &WasiEnv,
    exitcode: __wasi_exitcode_t,
) -> Result<__wasi_errno_t, WasiError> {
    super::thread_exit(env, exitcode)
}

pub(crate) fn sched_yield(env: &WasiEnv) -> Result<__wasi_errno_t, WasiError> {
    super::sched_yield(env)
}

pub(crate) fn getpid(env: &WasiEnv, ret_pid: WasmPtr<__wasi_pid_t, MemoryType>) -> __wasi_errno_t {
    super::getpid::<MemoryType>(env, ret_pid)
}

pub(crate) fn process_spawn(
    env: &WasiEnv,
    name: WasmPtr<u8, MemoryType>,
    name_len: MemoryOffset,
    chroot: __wasi_bool_t,
    args: WasmPtr<u8, MemoryType>,
    args_len: MemoryOffset,
    preopen: WasmPtr<u8, MemoryType>,
    preopen_len: MemoryOffset,
    stdin: __wasi_stdiomode_t,
    stdout: __wasi_stdiomode_t,
    stderr: __wasi_stdiomode_t,
    working_dir: WasmPtr<u8, MemoryType>,
    working_dir_len: MemoryOffset,
    ret_handles: WasmPtr<__wasi_bus_handles_t, MemoryType>,
) -> __bus_errno_t {
    super::process_spawn::<MemoryType>(
        env,
        name,
        name_len,
        chroot,
        args,
        args_len,
        preopen,
        preopen_len,
        stdin,
        stdout,
        stderr,
        working_dir,
        working_dir_len,
        ret_handles,
    )
}

pub(crate) fn bus_open_local(
    env: &WasiEnv,
    name: WasmPtr<u8, MemoryType>,
    name_len: MemoryOffset,
    reuse: __wasi_bool_t,
    ret_bid: WasmPtr<__wasi_bid_t, MemoryType>,
) -> __bus_errno_t {
    super::bus_open_local::<MemoryType>(env, name, name_len, reuse, ret_bid)
}

pub(crate) fn bus_open_remote(
    env: &WasiEnv,
    name: WasmPtr<u8, MemoryType>,
    name_len: MemoryOffset,
    reuse: __wasi_bool_t,
    instance: WasmPtr<u8, MemoryType>,
    instance_len: MemoryOffset,
    token: WasmPtr<u8, MemoryType>,
    token_len: MemoryOffset,
    ret_bid: WasmPtr<__wasi_bid_t, MemoryType>,
) -> __bus_errno_t {
    super::bus_open_remote::<MemoryType>(
        env,
        name,
        name_len,
        reuse,
        instance,
        instance_len,
        token,
        token_len,
        ret_bid,
    )
}

pub(crate) fn bus_close(env: &WasiEnv, bid: __wasi_bid_t) -> __bus_errno_t {
    super::bus_close(env, bid)
}

pub(crate) fn bus_call(
    env: &WasiEnv,
    bid: __wasi_bid_t,
    keep_alive: __wasi_bool_t,
    topic: WasmPtr<u8, MemoryType>,
    topic_len: MemoryOffset,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    ret_cid: WasmPtr<__wasi_cid_t, MemoryType>,
) -> __bus_errno_t {
    super::bus_call::<MemoryType>(
        env, bid, keep_alive, topic, topic_len, format, buf, buf_len, ret_cid,
    )
}

pub(crate) fn bus_subcall(
    env: &WasiEnv,
    parent: __wasi_cid_t,
    keep_alive: __wasi_bool_t,
    topic: WasmPtr<u8, MemoryType>,
    topic_len: MemoryOffset,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    ret_cid: WasmPtr<__wasi_cid_t, MemoryType>,
) -> __bus_errno_t {
    super::bus_subcall::<MemoryType>(
        env, parent, keep_alive, topic, topic_len, format, buf, buf_len, ret_cid,
    )
}

pub(crate) fn bus_poll(
    env: &WasiEnv,
    timeout: __wasi_timestamp_t,
    events: WasmPtr<u8, MemoryType>,
    nevents: MemoryOffset,
    malloc: WasmPtr<u8, MemoryType>,
    malloc_len: MemoryOffset,
    ret_nevents: WasmPtr<MemoryOffset, MemoryType>,
) -> __bus_errno_t {
    super::bus_poll::<MemoryType>(
        env,
        timeout,
        events,
        nevents,
        malloc,
        malloc_len,
        ret_nevents,
    )
}

pub(crate) fn call_reply(
    env: &WasiEnv,
    cid: __wasi_cid_t,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
) -> __bus_errno_t {
    super::call_reply::<MemoryType>(env, cid, format, buf, buf_len)
}

pub(crate) fn call_fault(env: &WasiEnv, cid: __wasi_cid_t, fault: __bus_errno_t) -> __bus_errno_t {
    super::call_fault(env, cid, fault)
}

pub(crate) fn call_close(env: &WasiEnv, cid: __wasi_cid_t) -> __bus_errno_t {
    super::call_close(env, cid)
}

pub(crate) fn port_bridge(
    env: &WasiEnv,
    network: WasmPtr<u8, MemoryType>,
    network_len: MemoryOffset,
    token: WasmPtr<u8, MemoryType>,
    token_len: MemoryOffset,
    security: __wasi_streamsecurity_t,
) -> __wasi_errno_t {
    super::port_bridge::<MemoryType>(env, network, network_len, token, token_len, security)
}

pub(crate) fn port_unbridge(env: &WasiEnv) -> __wasi_errno_t {
    super::port_unbridge(env)
}

pub(crate) fn port_dhcp_acquire(env: &WasiEnv) -> __wasi_errno_t {
    super::port_dhcp_acquire(env)
}

pub(crate) fn port_addr_add(
    env: &WasiEnv,
    addr: WasmPtr<__wasi_cidr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_addr_add::<MemoryType>(env, addr)
}

pub(crate) fn port_addr_remove(
    env: &WasiEnv,
    addr: WasmPtr<__wasi_addr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_addr_remove::<MemoryType>(env, addr)
}

pub(crate) fn port_addr_clear(env: &WasiEnv) -> __wasi_errno_t {
    super::port_addr_clear(env)
}

pub(crate) fn port_addr_list(
    env: &WasiEnv,
    addrs: WasmPtr<__wasi_cidr_t, MemoryType>,
    naddrs: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::port_addr_list::<MemoryType>(env, addrs, naddrs)
}

pub(crate) fn port_mac(
    env: &WasiEnv,
    ret_mac: WasmPtr<__wasi_hardwareaddress_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_mac::<MemoryType>(env, ret_mac)
}

pub(crate) fn port_gateway_set(
    env: &WasiEnv,
    ip: WasmPtr<__wasi_addr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_gateway_set::<MemoryType>(env, ip)
}

pub(crate) fn port_route_add(
    env: &WasiEnv,
    cidr: WasmPtr<__wasi_cidr_t, MemoryType>,
    via_router: WasmPtr<__wasi_addr_t, MemoryType>,
    preferred_until: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
    expires_at: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_route_add::<MemoryType>(env, cidr, via_router, preferred_until, expires_at)
}

pub(crate) fn port_route_remove(
    env: &WasiEnv,
    ip: WasmPtr<__wasi_addr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_route_remove::<MemoryType>(env, ip)
}

pub(crate) fn port_route_clear(env: &WasiEnv) -> __wasi_errno_t {
    super::port_route_clear(env)
}

pub(crate) fn port_route_list(
    env: &WasiEnv,
    routes: WasmPtr<__wasi_route_t, MemoryType>,
    nroutes: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::port_route_list::<MemoryType>(env, routes, nroutes)
}

pub(crate) fn ws_connect(
    env: &WasiEnv,
    url: WasmPtr<u8, MemoryType>,
    url_len: MemoryOffset,
    ret_sock: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::ws_connect::<MemoryType>(env, url, url_len, ret_sock)
}

pub(crate) fn http_request(
    env: &WasiEnv,
    url: WasmPtr<u8, MemoryType>,
    url_len: MemoryOffset,
    method: WasmPtr<u8, MemoryType>,
    method_len: MemoryOffset,
    headers: WasmPtr<u8, MemoryType>,
    headers_len: MemoryOffset,
    gzip: __wasi_bool_t,
    ret_handles: WasmPtr<__wasi_http_handles_t, MemoryType>,
) -> __wasi_errno_t {
    super::http_request::<MemoryType>(
        env,
        url,
        url_len,
        method,
        method_len,
        headers,
        headers_len,
        gzip,
        ret_handles,
    )
}

pub(crate) fn http_status(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    status: WasmPtr<__wasi_http_status_t, MemoryType>,
    status_text: WasmPtr<u8, MemoryType>,
    status_text_len: WasmPtr<MemoryOffset, MemoryType>,
    headers: WasmPtr<u8, MemoryType>,
    headers_len: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::http_status::<MemoryType>(env, sock, status)
}

pub(crate) fn sock_status(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    ret_status: WasmPtr<__wasi_sockstatus_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_status::<MemoryType>(env, sock, ret_status)
}

pub(crate) fn sock_addr_local(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    ret_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_addr_local::<MemoryType>(env, sock, ret_addr)
}

pub(crate) fn sock_addr_peer(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    ro_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_addr_peer::<MemoryType>(env, sock, ro_addr)
}

pub(crate) fn sock_open(
    env: &WasiEnv,
    af: __wasi_addressfamily_t,
    ty: __wasi_socktype_t,
    pt: __wasi_sockproto_t,
    ro_sock: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_open::<MemoryType>(env, af, ty, pt, ro_sock)
}

pub(crate) fn sock_set_opt_flag(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    flag: __wasi_bool_t,
) -> __wasi_errno_t {
    super::sock_set_opt_flag(env, sock, opt, flag)
}

pub(crate) fn sock_get_opt_flag(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    ret_flag: WasmPtr<__wasi_bool_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_get_opt_flag::<MemoryType>(env, sock, opt, ret_flag)
}

pub fn sock_set_opt_time(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    time: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_set_opt_time(env, sock, opt, time)
}

pub fn sock_get_opt_time(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    ret_time: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_get_opt_time(env, sock, opt, ret_time)
}

pub fn sock_set_opt_size(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    size: __wasi_filesize_t,
) -> __wasi_errno_t {
    super::sock_set_opt_size(env, sock, opt, size)
}

pub fn sock_get_opt_size(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    ret_size: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_get_opt_size(env, sock, opt, ret_size)
}

pub(crate) fn sock_join_multicast_v4(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
    iface: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_join_multicast_v4::<MemoryType>(env, sock, multiaddr, iface)
}

pub(crate) fn sock_leave_multicast_v4(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
    iface: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_leave_multicast_v4::<MemoryType>(env, sock, multiaddr, iface)
}

pub(crate) fn sock_join_multicast_v6(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, MemoryType>,
    iface: u32,
) -> __wasi_errno_t {
    super::sock_join_multicast_v6::<MemoryType>(env, sock, multiaddr, iface)
}

pub(crate) fn sock_leave_multicast_v6(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, MemoryType>,
    iface: u32,
) -> __wasi_errno_t {
    super::sock_leave_multicast_v6::<MemoryType>(env, sock, multiaddr, iface)
}

pub(crate) fn sock_bind(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_bind::<MemoryType>(env, sock, addr)
}

pub(crate) fn sock_listen(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    backlog: MemoryOffset,
) -> __wasi_errno_t {
    super::sock_listen::<MemoryType>(env, sock, backlog)
}

pub(crate) fn sock_accept(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    fd_flags: __wasi_fdflags_t,
    ro_fd: WasmPtr<__wasi_fd_t, MemoryType>,
    ro_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_accept::<MemoryType>(env, sock, fd_flags, ro_fd, ro_addr)
}

pub(crate) fn sock_connect(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_connect::<MemoryType>(env, sock, addr)
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

pub(crate) fn sock_recv_from(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    ri_data: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    ri_data_len: MemoryOffset,
    ri_flags: __wasi_riflags_t,
    ro_data_len: WasmPtr<MemoryOffset, MemoryType>,
    ro_flags: WasmPtr<__wasi_roflags_t, MemoryType>,
    ro_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_recv_from::<MemoryType>(
        env,
        sock,
        ri_data,
        ri_data_len,
        ri_flags,
        ro_data_len,
        ro_flags,
        ro_addr,
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

pub(crate) fn sock_send_to(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    si_data_len: MemoryOffset,
    si_flags: __wasi_siflags_t,
    addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
    ret_data_len: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_send_to::<MemoryType>(
        env,
        sock,
        si_data,
        si_data_len,
        si_flags,
        addr,
        ret_data_len,
    )
}

pub(crate) fn sock_send_file(
    env: &WasiEnv,
    out_fd: __wasi_fd_t,
    in_fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    count: __wasi_filesize_t,
    ret_sent: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    unsafe { super::sock_send_file::<MemoryType>(env, out_fd, in_fd, offset, count, ret_sent) }
}

pub(crate) fn sock_shutdown(
    env: &WasiEnv,
    sock: __wasi_fd_t,
    how: __wasi_sdflags_t,
) -> __wasi_errno_t {
    super::sock_shutdown(env, sock, how)
}

pub(crate) fn resolve(
    env: &WasiEnv,
    host: WasmPtr<u8, MemoryType>,
    host_len: MemoryOffset,
    port: u16,
    ips: WasmPtr<__wasi_addr_t, MemoryType>,
    nips: MemoryOffset,
    ret_nips: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::resolve::<MemoryType>(env, host, host_len, port, ips, nips, ret_nips)
}
