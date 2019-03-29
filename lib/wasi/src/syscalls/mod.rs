#![allow(unused)]
pub mod types;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix;
#[cfg(any(target_os = "windows"))]
pub mod windows;

use self::types::*;
use crate::{
    ptr::{Array, WasmPtr},
    state::WasiState,
};
use rand::{thread_rng, Rng};
use wasmer_runtime_core::{memory::Memory, vm::Ctx};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::*;

#[cfg(any(target_os = "windows"))]
pub use windows::*;

#[allow(clippy::mut_from_ref)]
fn get_wasi_state(ctx: &Ctx) -> &mut WasiState {
    unsafe { &mut *(ctx.data as *mut WasiState) }
}

#[must_use]
fn write_buffer_array(
    memory: &Memory,
    from: &[Vec<u8>],
    ptr_buffer: WasmPtr<WasmPtr<u8, Array>, Array>,
    buffer: WasmPtr<u8, Array>,
) -> __wasi_errno_t {
    let ptrs = if let Some(cells) = ptr_buffer.deref(memory, 0, from.len() as u32) {
        cells
    } else {
        return __WASI_EOVERFLOW;
    };

    let mut current_buffer_offset = 0;
    for ((i, sub_buffer), ptr) in from.iter().enumerate().zip(ptrs.iter()) {
        ptr.set(WasmPtr::new(buffer.offset() + current_buffer_offset));

        let cells = if let Some(cells) =
            buffer.deref(memory, current_buffer_offset, sub_buffer.len() as u32)
        {
            cells
        } else {
            return __WASI_EOVERFLOW;
        };

        for (cell, &byte) in cells.iter().zip(sub_buffer.iter()) {
            cell.set(byte);
        }
        current_buffer_offset += sub_buffer.len() as u32;
    }

    __WASI_ESUCCESS
}

/// ### `args_get()`
/// Read command-line argument data.
/// The sizes of the buffers should match that returned by [`args_sizes_get()`](#args_sizes_get).
/// Inputs:
/// - `char **argv`
///     A pointer to a buffer to write the argument pointers.
/// - `char *argv_buf`
///     A pointer to a buffer to write the argument string data.
///
pub fn args_get(
    ctx: &mut Ctx,
    argv: WasmPtr<WasmPtr<u8, Array>, Array>,
    argv_buf: WasmPtr<u8, Array>,
) -> __wasi_errno_t {
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    write_buffer_array(memory, &*state.args, argv, argv_buf)
}

/// ### `args_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *argc`
///     The number of arguments.
/// - `size_t *argv_buf_size`
///     The size of the argument string data.
pub fn args_sizes_get(
    ctx: &mut Ctx,
    argc: WasmPtr<u32>,
    argv_buf_size: WasmPtr<u32>,
) -> __wasi_errno_t {
    let memory = ctx.memory(0);

    if let (Some(argc), Some(argv_buf_size)) = (argc.deref(memory), argv_buf_size.deref(memory)) {
        let state = get_wasi_state(ctx);

        argc.set(state.args.len() as u32);
        argv_buf_size.set(state.args.iter().map(|v| v.len() as u32).sum());

        __WASI_ESUCCESS
    } else {
        __WASI_EOVERFLOW
    }
}

pub fn clock_res_get(
    ctx: &mut Ctx,
    clock_id: __wasi_clockid_t,
    resolution: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let memory = ctx.memory(0);

    if let Some(out_addr) = resolution.deref(memory) {
        platform_clock_res_get(clock_id, out_addr)
    } else {
        __WASI_EFAULT
    }
}

pub fn clock_time_get(
    ctx: &mut Ctx,
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    let memory = ctx.memory(0);

    if let Some(out_addr) = time.deref(memory) {
        platform_clock_time_get(clock_id, precision, out_addr)
    } else {
        __WASI_EFAULT
    }
}

/// ### `environ_get()`
/// Read environment variable data.
/// The sizes of the buffers should match that returned by [`environ_sizes_get()`](#environ_sizes_get).
/// Inputs:
/// - `char **environ`
///     A pointer to a buffer to write the environment variable pointers.
/// - `char *environ_buf`
///     A pointer to a buffer to write the environment variable string data.
pub fn environ_get(
    ctx: &mut Ctx,
    environ: WasmPtr<WasmPtr<u8, Array>, Array>,
    environ_buf: WasmPtr<u8, Array>,
) -> __wasi_errno_t {
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    write_buffer_array(memory, &*state.args, environ, environ_buf)
}

/// ### `environ_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *environ_count`
///     The number of environment variables.
/// - `size_t *environ_buf_size`
///     The size of the environment variable string data.
pub fn environ_sizes_get(
    ctx: &mut Ctx,
    environ_count: WasmPtr<u32>,
    environ_buf_size: WasmPtr<u32>,
) -> __wasi_errno_t {
    let memory = ctx.memory(0);

    if let (Some(environ_count), Some(environ_buf_size)) =
        (environ_count.deref(memory), environ_buf_size.deref(memory))
    {
        let state = get_wasi_state(ctx);

        environ_count.set(state.envs.len() as u32);
        environ_buf_size.set(state.envs.iter().map(|v| v.len() as u32).sum());

        __WASI_ESUCCESS
    } else {
        __WASI_EOVERFLOW
    }
}

pub fn fd_advise(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
    advice: __wasi_advice_t,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_allocate(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_close(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_datasync(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_fdstat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_fdstat_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_fdstat_set_flags(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    flags: __wasi_fdflags_t,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_fdstat_set_rights(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_filestat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_filestat_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_filestat_set_size(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    st_size: __wasi_filesize_t,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_filestat_set_times(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_pread(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t, Array>,
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nread: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_prestat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_fdstat_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_prestat_dir_name(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_pwrite(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t, Array>,
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nwritten: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_read(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t, Array>,
    iovs_len: u32,
    nread: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_readdir(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf: WasmPtr<u8, Array>,
    buf_len: u32,
    cookie: __wasi_dircookie_t,
    bufused: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_renumber(ctx: &mut Ctx, from: __wasi_fd_t, to: __wasi_fd_t) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_seek(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: __wasi_filedelta_t,
    whence: __wasi_whence_t,
    newoffset: WasmPtr<__wasi_filesize_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_sync(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_tell(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: WasmPtr<__wasi_filesize_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn fd_write(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t, Array>,
    iovs_len: u32,
    nwritten: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_create_directory(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_filestat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
    buf: WasmPtr<__wasi_filestat_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_filestat_set_times(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_link(
    ctx: &mut Ctx,
    old_fd: __wasi_fd_t,
    old_flags: __wasi_lookupflags_t,
    old_path: WasmPtr<u8, Array>,
    old_path_len: u32,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, Array>,
    new_path_len: u32,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_open(
    ctx: &mut Ctx,
    dirfd: __wasi_fd_t,
    dirflags: __wasi_lookupflags_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
    o_flags: __wasi_oflags_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
    fs_flags: __wasi_fdflags_t,
    fd: WasmPtr<__wasi_fd_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_readlink(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
    buf: WasmPtr<u8>,
    buf_len: u32,
    bufused: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_remove_directory(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_rename(
    ctx: &mut Ctx,
    old_fd: __wasi_fd_t,
    old_path: WasmPtr<u8, Array>,
    old_path_len: u32,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8, Array>,
    new_path_len: u32,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_symlink(
    ctx: &mut Ctx,
    old_path: WasmPtr<u8, Array>,
    old_path_len: u32,
    fd: __wasi_fd_t,
    new_path: WasmPtr<u8, Array>,
    new_path_len: u32,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn path_unlink_file(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn poll_oneoff(
    ctx: &mut Ctx,
    in_: WasmPtr<__wasi_subscription_t, Array>,
    out_: WasmPtr<__wasi_event_t, Array>,
    nsubscriptions: u32,
    nevents: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn proc_exit(ctx: &mut Ctx, rval: __wasi_exitcode_t) {
    unimplemented!()
}
pub fn proc_raise(ctx: &mut Ctx, sig: __wasi_signal_t) -> __wasi_errno_t {
    unimplemented!()
}

/// ### `random_get()`
/// Fill buffer with high-quality random data.  This function may be slow and block
/// Inputs:
/// - `void *buf`
///     A pointer to a buffer where the random bytes will be written
/// - `size_t buf_len`
///     The number of bytes that will be written
pub fn random_get(ctx: &mut Ctx, buf: WasmPtr<u8, Array>, buf_len: u32) -> __wasi_errno_t {
    let mut rng = thread_rng();
    let memory = ctx.memory(0);

    if let Some(buf) = buf.deref(memory, 0, buf_len) {
        for i in 0..(buf_len as usize) {
            let random_byte = rng.gen::<u8>();
            buf[i].set(random_byte);
        }
    } else {
        return __WASI_EFAULT;
    }

    __WASI_ESUCCESS
}

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(ctx: &mut Ctx) -> __wasi_errno_t {
    __WASI_ESUCCESS
}

pub fn sock_recv(
    ctx: &mut Ctx,
    sock: __wasi_fd_t,
    ri_data: WasmPtr<__wasi_iovec_t, Array>,
    ri_data_len: u32,
    ri_flags: __wasi_riflags_t,
    ro_datalen: WasmPtr<u32>,
    ro_flags: WasmPtr<__wasi_roflags_t>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn sock_send(
    ctx: &mut Ctx,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t, Array>,
    si_data_len: u32,
    si_flags: __wasi_siflags_t,
    so_datalen: WasmPtr<u32>,
) -> __wasi_errno_t {
    unimplemented!()
}
pub fn sock_shutdown(ctx: &mut Ctx, sock: __wasi_fd_t, how: __wasi_sdflags_t) -> __wasi_errno_t {
    unimplemented!()
}
