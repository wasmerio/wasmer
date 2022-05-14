#![deny(dead_code)]
use wasmer_wasi_types::*;
use wasmer::{Memory, WasmPtr, WasmSlice, MemorySize, Memory64};
use crate::{WasiThread, WasiState, WasiError};

type MemoryType = Memory64;
type MemoryOffset = u64;

pub(crate) fn args_get(
    thread: &WasiThread,
    argv: WasmPtr<WasmPtr<u8, MemoryType>, MemoryType>,
    argv_buf: WasmPtr<u8, MemoryType>,
) -> __wasi_errno_t {
    super::args_get::<MemoryType>(thread, argv, argv_buf)
}

pub(crate) fn args_sizes_get(
    thread: &WasiThread,
    argc: WasmPtr<MemoryOffset, MemoryType>,
    argv_buf_size: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::args_sizes_get::<MemoryType>(thread, argc, argv_buf_size)
}

pub(crate) fn clock_res_get(
    thread: &WasiThread,
    clock_id: __wasi_clockid_t,
    resolution: WasmPtr<__wasi_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::clock_res_get::<MemoryType>(thread, clock_id, resolution)
}

pub(crate) fn clock_time_get(
    thread: &WasiThread,
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::clock_time_get::<MemoryType>(thread, clock_id, precision, time)
}

pub(crate) fn environ_get(
    thread: &WasiThread,
    environ: WasmPtr<WasmPtr<u8, MemoryType>, MemoryType>,
    environ_buf: WasmPtr<u8, MemoryType>,
) -> __wasi_errno_t {
    super::environ_get::<MemoryType>(thread, environ, environ_buf)
}

pub(crate) fn environ_sizes_get(
    thread: &WasiThread,
    environ_count: WasmPtr<MemoryOffset, MemoryType>,
    environ_buf_size: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::environ_sizes_get::<MemoryType>(thread, environ_count, environ_buf_size)
}

pub(crate) fn fd_advise(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
    advice: __wasi_advice_t,
) -> __wasi_errno_t {
    super::fd_advise(thread, fd, offset, len, advice)
}

pub(crate) fn fd_allocate(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
) -> __wasi_errno_t {
    super::fd_allocate(thread, fd, offset, len)
}

pub(crate) fn fd_close(thread: &WasiThread, fd: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_close(thread, fd)
}

pub(crate) fn fd_datasync(thread: &WasiThread, fd: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_datasync(thread, fd)
}

pub(crate) fn fd_fdstat_get(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf_ptr: WasmPtr<__wasi_fdstat_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_fdstat_get::<MemoryType>(thread, fd, buf_ptr)
}

pub(crate) fn fd_fdstat_set_flags(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    flags: __wasi_fdflags_t,
) -> __wasi_errno_t {
    super::fd_fdstat_set_flags(thread, fd, flags)
}

pub(crate) fn fd_fdstat_set_rights(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
) -> __wasi_errno_t {
    super::fd_fdstat_set_rights(thread, fd, fs_rights_base, fs_rights_inheriting)
}

pub(crate) fn fd_filestat_get(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_filestat_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_filestat_get::<MemoryType>(thread, fd, buf)
}

pub(crate) fn fd_filestat_set_size(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    st_size: __wasi_filesize_t,
) -> __wasi_errno_t {
    super::fd_filestat_set_size(thread, fd, st_size)
}

pub(crate) fn fd_filestat_set_times(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    super::fd_filestat_set_times(thread, fd, st_atim, st_mtim, fst_flags)
}

pub(crate) fn fd_pread(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    offset: __wasi_filesize_t,
    nread: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_pread::<MemoryType>(thread, fd, iovs, iovs_len, offset, nread)
}

pub(crate) fn fd_prestat_get(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_prestat_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_prestat_get::<MemoryType>(thread, fd, buf)
}

pub(crate) fn fd_prestat_dir_name(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::fd_prestat_dir_name::<MemoryType>(thread, fd, path, path_len)
}

pub(crate) fn fd_pwrite(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    offset: __wasi_filesize_t,
    nwritten: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_pwrite::<MemoryType>(thread, fd, iovs, iovs_len, offset, nwritten)
}

pub(crate) fn fd_read(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    nread: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_read::<MemoryType>(thread, fd, iovs, iovs_len, nread)
}

pub(crate) fn fd_readdir(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    cookie: __wasi_dircookie_t,
    bufused: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::fd_readdir::<MemoryType>(thread, fd, buf, buf_len, cookie, bufused)
}

pub(crate) fn fd_renumber(thread: &WasiThread, from: __wasi_fd_t, to: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_renumber(thread, from, to)
}

pub(crate) fn fd_seek(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    offset: __wasi_filedelta_t,
    whence: __wasi_whence_t,
    newoffset: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_seek::<MemoryType>(thread, fd, offset, whence, newoffset)
}

pub(crate) fn fd_sync(thread: &WasiThread, fd: __wasi_fd_t) -> __wasi_errno_t {
    super::fd_sync(thread, fd)
}

pub(crate) fn fd_tell(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    offset: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_tell::<MemoryType>(thread, fd, offset)
}

pub(crate) fn fd_write(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    iovs_len: MemoryOffset,
    nwritten: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::fd_write::<MemoryType>(thread, fd, iovs, iovs_len, nwritten)
}

pub(crate) fn path_create_directory(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_create_directory::<MemoryType>(thread, fd, path, path_len)
}

pub(crate) fn path_filestat_get(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    buf: WasmPtr<__wasi_filestat_t, MemoryType>,
) -> __wasi_errno_t {
    super::path_filestat_get::<MemoryType>(thread, fd, flags, path, path_len, buf)
}

pub(crate) fn path_filestat_set_times(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    super::path_filestat_set_times::<MemoryType>(thread, fd, flags, path, path_len, st_atim, st_mtim, fst_flags)
}

pub(crate) fn path_link(
    thread: &WasiThread,
    old_fd: __wasi_fd_t,
    old_flags: __wasi_lookupflags_t,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_link::<MemoryType>(thread, old_fd, old_flags, old_path, old_path_len, new_fd, new_path, new_path_len)
}

pub(crate) fn path_open(
    thread: &WasiThread,
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
    super::path_open::<MemoryType>(thread, dirfd, dirflags, path, path_len, o_flags, fs_rights_base, fs_rights_inheriting, fs_flags, fd)
}

pub(crate) fn path_readlink(
    thread: &WasiThread,
    dir_fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    buf_used: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::path_readlink::<MemoryType>(thread, dir_fd, path, path_len, buf, buf_len, buf_used)
}

pub(crate) fn path_remove_directory(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_remove_directory::<MemoryType>(thread, fd, path, path_len)
}

pub(crate) fn path_rename(
    thread: &WasiThread,
    old_fd: __wasi_fd_t,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_rename::<MemoryType>(thread, old_fd, old_path, old_path_len, new_fd, new_path, new_path_len)
}

pub(crate) fn path_symlink(
    thread: &WasiThread,
    old_path: WasmPtr<u8, MemoryType>,
    old_path_len: MemoryOffset,
    fd: __wasi_fd_t,
    new_path: WasmPtr<u8, MemoryType>,
    new_path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_symlink::<MemoryType>(thread, old_path, old_path_len, fd, new_path, new_path_len)
}

pub(crate) fn path_unlink_file(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::path_unlink_file::<MemoryType>(thread, fd, path, path_len)
}

pub(crate) fn poll_oneoff(
    thread: &WasiThread,
    in_: WasmPtr<__wasi_subscription_t, MemoryType>,
    out_: WasmPtr<__wasi_event_t, MemoryType>,
    nsubscriptions: MemoryOffset,
    nevents: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::poll_oneoff::<MemoryType>(thread, in_, out_, nsubscriptions, nevents)
}

pub(crate) fn proc_exit(thread: &WasiThread, code: __wasi_exitcode_t) -> Result<(), WasiError> {
    super::proc_exit(thread, code)
}

pub(crate) fn proc_raise(thread: &WasiThread, sig: __wasi_signal_t) -> __wasi_errno_t {
    super::proc_raise(thread, sig)
}

pub(crate) fn random_get(
    thread: &WasiThread,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset
) -> __wasi_errno_t {
    super::random_get::<MemoryType>(thread, buf, buf_len)
}

pub(crate) fn fd_dup(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    ret_fd: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_dup::<MemoryType>(thread, fd, ret_fd)
}

pub(crate) fn fd_event(
    thread: &WasiThread,
    initial_val: u64,
    flags: __wasi_eventfdflags,
    ret_fd: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_event(thread, initial_val, flags, ret_fd)
}

pub(crate) fn fd_pipe(
    thread: &WasiThread,
    ro_fd1: WasmPtr<__wasi_fd_t, MemoryType>,
    ro_fd2: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::fd_pipe::<MemoryType>(thread, ro_fd1, ro_fd2)
}

pub(crate) fn tty_get(
    thread: &WasiThread,
    tty_state: WasmPtr<__wasi_tty_t, MemoryType>,
) -> __wasi_errno_t {
    super::tty_get::<MemoryType>(thread, tty_state)
}

pub(crate) fn tty_set(
    thread: &WasiThread,
    tty_state: WasmPtr<__wasi_tty_t, MemoryType>,
) -> __wasi_errno_t {
    super::tty_set::<MemoryType>(thread, tty_state)
}

pub(crate) fn getcwd(
    thread: &WasiThread,
    path: WasmPtr<u8, MemoryType>,
    path_len: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::getcwd::<MemoryType>(thread, path, path_len)
}

pub(crate) fn chdir(
    thread: &WasiThread,
    path: WasmPtr<u8, MemoryType>,
    path_len: MemoryOffset,
) -> __wasi_errno_t {
    super::chdir::<MemoryType>(thread, path, path_len)
}

pub(crate) fn thread_spawn(
    thread: &WasiThread,
    method: WasmPtr<u8, MemoryType>,
    method_len: MemoryOffset,
    user_data: u64,
    reactor: __wasi_bool_t,
    ret_tid: WasmPtr<__wasi_tid_t, MemoryType>,
) -> __wasi_errno_t {
    super::thread_spawn::<MemoryType>(thread, method, method_len, user_data, reactor, ret_tid)
}

pub(crate) fn thread_sleep(
    thread: &WasiThread,
    duration: __wasi_timestamp_t,
) -> Result<__wasi_errno_t, WasiError> {
    super::thread_sleep(thread, duration)
}

pub(crate) fn thread_id(
    thread: &WasiThread,
    ret_tid: WasmPtr<__wasi_tid_t, MemoryType>,
) -> __wasi_errno_t {
    super::thread_id::<MemoryType>(thread, ret_tid)
}

pub(crate) fn thread_join(
    thread: &WasiThread,
    tid: __wasi_tid_t,
) -> __wasi_errno_t {
    super::thread_join(thread, tid)
}

pub(crate) fn thread_parallelism(
    thread: &WasiThread,
    ret_parallelism: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::thread_parallelism::<MemoryType>(thread, ret_parallelism)
}

pub(crate) fn thread_exit(
    thread: &WasiThread,
    exitcode: __wasi_exitcode_t,
) -> Result<__wasi_errno_t, WasiError> {
    super::thread_exit(thread, exitcode)
}

pub(crate) fn sched_yield(thread: &WasiThread) -> Result<__wasi_errno_t, WasiError> {
    super::sched_yield(thread)
}

pub(crate) fn getpid(
    thread: &WasiThread,
    ret_pid: WasmPtr<__wasi_pid_t, MemoryType>,
) -> __wasi_errno_t {
    super::getpid::<MemoryType>(thread, ret_pid)
}

pub(crate) fn bus_spawn_local(
    thread: &WasiThread,
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
) -> __wasi_errno_t {
    super::bus_spawn_local::<MemoryType>(thread, name, name_len, chroot, args, args_len, preopen, preopen_len, stdin, stdout, stderr, working_dir, working_dir_len, ret_handles)
}

pub(crate) fn bus_spawn_remote(
    thread: &WasiThread,
    name: WasmPtr<u8, MemoryType>,
    name_len: MemoryOffset,
    chroot: __wasi_bool_t,
    args: WasmPtr<u8, MemoryType>,
    args_len: MemoryOffset,
    preopen: WasmPtr<u8, MemoryType>,
    preopen_len: MemoryOffset,
    working_dir: WasmPtr<u8, MemoryType>,
    working_dir_len: MemoryOffset,
    stdin: __wasi_stdiomode_t,
    stdout: __wasi_stdiomode_t,
    stderr: __wasi_stdiomode_t,
    instance: WasmPtr<u8, MemoryType>,
    instance_len: MemoryOffset,
    token: WasmPtr<u8, MemoryType>,
    token_len: MemoryOffset,
    ret_handles: WasmPtr<__wasi_bus_handles_t, MemoryType>,
) -> __wasi_errno_t {
    super::bus_spawn_remote::<MemoryType>(thread, name, name_len, chroot, args, args_len, preopen, preopen_len, working_dir, working_dir_len, stdin, stdout, stderr, instance, instance_len, token, token_len, ret_handles)
}

pub(crate) fn bus_close(
    thread: &WasiThread,
    bid: __wasi_bid_t,
) -> __wasi_errno_t {
    super::bus_close(thread, bid)
}

pub(crate) fn bus_invoke(
    thread: &WasiThread,
    bid: __wasi_bid_t,
    cid: WasmPtr<__wasi_option_cid_t, MemoryType>,
    keep_alive: __wasi_bool_t,
    topic: WasmPtr<u8, MemoryType>,
    topic_len: MemoryOffset,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    ret_cid: WasmPtr<__wasi_cid_t, MemoryType>,
) -> __wasi_errno_t {
    super::bus_invoke::<MemoryType>(thread, bid, cid, keep_alive, topic, topic_len, format, buf, buf_len, ret_cid)
}

pub(crate) fn bus_fault(
    thread: &WasiThread,
    cid: __wasi_cid_t,
    fault: __bus_errno_t,
) -> __wasi_errno_t {
    super::bus_fault(thread, cid, fault)
}

pub(crate) fn bus_drop(
    thread: &WasiThread,
    cid: __wasi_cid_t,
) -> __wasi_errno_t {
    super::bus_drop(thread, cid)
}

pub(crate) fn bus_reply(
    thread: &WasiThread,
    cid: __wasi_cid_t,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
) -> __wasi_errno_t {
    super::bus_reply::<MemoryType>(thread, cid, format, buf, buf_len)
}

pub(crate) fn bus_callback(
    thread: &WasiThread,
    cid: __wasi_cid_t,
    topic: WasmPtr<u8, MemoryType>,
    topic_len: MemoryOffset,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
) -> __wasi_errno_t {
    super::bus_callback::<MemoryType>(thread, cid, topic, topic_len, format, buf, buf_len)
}

pub(crate) fn bus_listen(
    thread: &WasiThread,
    parent: WasmPtr<__wasi_option_cid_t, MemoryType>,
    topic: WasmPtr<u8, MemoryType>,
    topic_len: MemoryOffset,
) -> __wasi_errno_t {
    super::bus_listen::<MemoryType>(thread, parent, topic, topic_len)
}

pub(crate) fn bus_poll(
    thread: &WasiThread,
    bid: WasmPtr<__wasi_option_bid_t, MemoryType>,
    timeout: WasmPtr<__wasi_timestamp_t, MemoryType>,
    events: WasmPtr<u8, MemoryType>,
    nevents: MemoryOffset,
    ret_nevents: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::bus_poll::<MemoryType>(thread, bid, timeout, events, nevents, ret_nevents)
}

pub(crate) fn bus_poll_data(
    thread: &WasiThread,
    bid: WasmPtr<__wasi_option_bid_t, MemoryType>,
    timeout: WasmPtr<__wasi_timestamp_t, MemoryType>,
    topic: WasmPtr<u8, MemoryType>,
    topic_len: MemoryOffset,
    buf: WasmPtr<u8, MemoryType>,
    buf_len: MemoryOffset,
    ret_evt: WasmPtr<__wasi_busevent_data_t<MemoryType>, MemoryType>,
) -> __wasi_errno_t {
    super::bus_poll_data::<MemoryType>(thread, bid, timeout, topic, topic_len, buf, buf_len, ret_evt)
}

pub(crate) fn port_bridge(
    thread: &WasiThread,
    network: WasmPtr<u8, MemoryType>,
    network_len: MemoryOffset,
    token: WasmPtr<u8, MemoryType>,
    token_len: MemoryOffset,
    security: __wasi_streamsecurity_t,
) -> __wasi_errno_t {
    super::port_bridge::<MemoryType>(thread, network, network_len, token, token_len, security)
}

pub(crate) fn port_unbridge(
    thread: &WasiThread,
) -> __wasi_errno_t {
    super::port_unbridge(thread)
}

pub(crate) fn port_dhcp_acquire(
    thread: &WasiThread,
) -> __wasi_errno_t {
    super::port_dhcp_acquire(thread)
}

pub(crate) fn port_addr_add(
    thread: &WasiThread,
    addr: WasmPtr<__wasi_cidr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_addr_add::<MemoryType>(thread, addr)
}

pub(crate) fn port_addr_remove(
    thread: &WasiThread,
    addr: WasmPtr<__wasi_addr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_addr_remove::<MemoryType>(thread, addr)
}

pub(crate) fn port_addr_clear(
    thread: &WasiThread,
) -> __wasi_errno_t {
    super::port_addr_clear(thread)
}

pub(crate) fn port_addr_list(
    thread: &WasiThread,
    addrs: WasmPtr<__wasi_cidr_t, MemoryType>,
    naddrs: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::port_addr_list::<MemoryType>(thread, addrs, naddrs)
}

pub(crate) fn port_mac(
    thread: &WasiThread,
    ret_mac: WasmPtr<__wasi_hardwareaddress_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_mac::<MemoryType>(thread, ret_mac)
}

pub(crate) fn port_gateway_set(
    thread: &WasiThread,
    ip: WasmPtr<__wasi_addr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_gateway_set::<MemoryType>(thread, ip)
}

pub(crate) fn port_route_add(
    thread: &WasiThread,
    cidr: WasmPtr<__wasi_cidr_t, MemoryType>,
    via_router: WasmPtr<__wasi_addr_t, MemoryType>,
    preferred_until: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
    expires_at: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_route_add::<MemoryType>(thread, cidr, via_router, preferred_until, expires_at)
}

pub(crate) fn port_route_remove(
    thread: &WasiThread,
    ip: WasmPtr<__wasi_addr_t, MemoryType>,
) -> __wasi_errno_t {
    super::port_route_remove::<MemoryType>(thread, ip)
}

pub(crate) fn port_route_clear(
    thread: &WasiThread,
) -> __wasi_errno_t {
    super::port_route_clear(thread)
}

pub(crate) fn port_route_list(
    thread: &WasiThread,
    routes: WasmPtr<__wasi_route_t, MemoryType>,
    nroutes: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::port_route_list::<MemoryType>(thread, routes, nroutes)
}

pub(crate) fn ws_connect(
    thread: &WasiThread,
    url: WasmPtr<u8, MemoryType>,
    url_len: MemoryOffset,
    ret_sock: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::ws_connect::<MemoryType>(thread, url, url_len, ret_sock)
}

pub(crate) fn http_request(
    thread: &WasiThread,
    url: WasmPtr<u8, MemoryType>,
    url_len: MemoryOffset,
    method: WasmPtr<u8, MemoryType>,
    method_len: MemoryOffset,
    headers: WasmPtr<u8, MemoryType>,
    headers_len: MemoryOffset,
    gzip: __wasi_bool_t,
    ret_handles: WasmPtr<__wasi_http_handles_t, MemoryType>,
) -> __wasi_errno_t {
    super::http_request::<MemoryType>(thread, url, url_len, method, method_len, headers, headers_len, gzip, ret_handles)
}

pub(crate) fn http_status(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    status: WasmPtr<__wasi_http_status_t, MemoryType>,
    status_text: WasmPtr<u8, MemoryType>,
    status_text_len: WasmPtr<MemoryOffset, MemoryType>,
    headers: WasmPtr<u8, MemoryType>,
    headers_len: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::http_status::<MemoryType>(thread, sock, status)
}

pub(crate) fn sock_status(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    ret_status: WasmPtr<__wasi_sockstatus_t, MemoryType>
) -> __wasi_errno_t {
    super::sock_status::<MemoryType>(thread, sock, ret_status)
}

pub(crate) fn sock_addr_local(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    ret_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_addr_local::<MemoryType>(thread, sock, ret_addr)
}

pub(crate) fn sock_addr_peer(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    ro_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_addr_peer::<MemoryType>(thread, sock, ro_addr)
}

pub(crate) fn sock_open(
    thread: &WasiThread,
    af: __wasi_addressfamily_t,
    ty: __wasi_socktype_t,
    pt: __wasi_sockproto_t,
    ro_sock: WasmPtr<__wasi_fd_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_open::<MemoryType>(thread, af, ty, pt, ro_sock)
}

pub(crate) fn sock_set_opt_flag(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    flag: __wasi_bool_t,
) -> __wasi_errno_t {
    super::sock_set_opt_flag(thread, sock, opt, flag)
}

pub(crate) fn sock_get_opt_flag(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    ret_flag: WasmPtr<__wasi_bool_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_get_opt_flag::<MemoryType>(thread, sock, opt, ret_flag)
}

pub fn sock_set_opt_time(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    time: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_set_opt_time(thread, sock, opt, time)
}

pub fn sock_get_opt_time(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    ret_time: WasmPtr<__wasi_option_timestamp_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_get_opt_time(thread, sock, opt, ret_time)
}

pub fn sock_set_opt_size(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    size: __wasi_filesize_t,
) -> __wasi_errno_t {
    super::sock_set_opt_size(thread, sock, opt, size)
}

pub fn sock_get_opt_size(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    opt: __wasi_sockoption_t,
    ret_size: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_get_opt_size(thread, sock, opt, ret_size)
}

pub(crate) fn sock_join_multicast_v4(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
    iface: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_join_multicast_v4::<MemoryType>(thread, sock, multiaddr, iface)
}

pub(crate) fn sock_leave_multicast_v4(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
    iface: WasmPtr<__wasi_addr_ip4_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_leave_multicast_v4::<MemoryType>(thread, sock, multiaddr, iface)
}

pub(crate) fn sock_join_multicast_v6(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, MemoryType>,
    iface: u32,
) -> __wasi_errno_t {
    super::sock_join_multicast_v6::<MemoryType>(thread, sock, multiaddr, iface)
}

pub(crate) fn sock_leave_multicast_v6(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, MemoryType>,
    iface: u32,
) -> __wasi_errno_t {
    super::sock_leave_multicast_v6::<MemoryType>(thread, sock, multiaddr, iface)
}

pub(crate) fn sock_bind(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_bind::<MemoryType>(thread, sock, addr)
}

pub(crate) fn sock_listen(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    backlog: MemoryOffset,
) -> __wasi_errno_t {
    super::sock_listen::<MemoryType>(thread, sock, backlog)
}

pub(crate) fn sock_accept(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    fd_flags: __wasi_fdflags_t,
    ro_fd: WasmPtr<__wasi_fd_t, MemoryType>,
    ro_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_accept::<MemoryType>(thread, sock, fd_flags, ro_fd, ro_addr)
}

pub(crate) fn sock_connect(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> __wasi_errno_t {
    super::sock_connect::<MemoryType>(thread, sock, addr)
}

pub(crate) fn sock_recv(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    ri_data: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    ri_data_len: MemoryOffset,
    ri_flags: __wasi_riflags_t,
    ro_data_len: WasmPtr<MemoryOffset, MemoryType>,
    ro_flags: WasmPtr<__wasi_roflags_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_recv::<MemoryType>(thread, sock, ri_data, ri_data_len, ri_flags, ro_data_len, ro_flags)
}

pub(crate) fn sock_recv_from(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    ri_data: WasmPtr<__wasi_iovec_t<MemoryType>, MemoryType>,
    ri_data_len: MemoryOffset,
    ri_flags: __wasi_riflags_t,
    ro_data_len: WasmPtr<MemoryOffset, MemoryType>,
    ro_flags: WasmPtr<__wasi_roflags_t, MemoryType>,
    ro_addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_recv_from::<MemoryType>(thread, sock, ri_data, ri_data_len, ri_flags, ro_data_len, ro_flags, ro_addr)
}

pub(crate) fn sock_send(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    si_data_len: MemoryOffset,
    si_flags: __wasi_siflags_t,
    ret_data_len: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_send::<MemoryType>(thread, sock, si_data, si_data_len, si_flags, ret_data_len)
}

pub(crate) fn sock_send_to(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t<MemoryType>, MemoryType>,
    si_data_len: MemoryOffset,
    si_flags: __wasi_siflags_t,
    addr: WasmPtr<__wasi_addr_port_t, MemoryType>,
    ret_data_len: WasmPtr<MemoryOffset, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    super::sock_send_to::<MemoryType>(thread, sock, si_data, si_data_len, si_flags, addr, ret_data_len)
}

pub(crate) fn sock_send_file(
    thread: &WasiThread,
    out_fd: __wasi_fd_t,
    in_fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    count: __wasi_filesize_t,
    ret_sent: WasmPtr<__wasi_filesize_t, MemoryType>,
) -> Result<__wasi_errno_t, WasiError> {
    unsafe {
        super::sock_send_file::<MemoryType>(thread, out_fd, in_fd, offset, count, ret_sent)
    }
}

pub(crate) fn sock_shutdown(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    how: __wasi_sdflags_t,
) -> __wasi_errno_t {
    super::sock_shutdown(thread, sock, how)
}

pub(crate) fn resolve(
    thread: &WasiThread,
    host: WasmPtr<u8, MemoryType>,
    host_len: MemoryOffset,
    port: u16,
    ips: WasmPtr<__wasi_addr_t, MemoryType>,
    nips: MemoryOffset,
    ret_nips: WasmPtr<MemoryOffset, MemoryType>,
) -> __wasi_errno_t {
    super::resolve::<MemoryType>(thread, host, host_len, port, ips, nips, ret_nips)
}
