#![allow(unused)]
pub mod types;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix;
#[cfg(any(target_os = "windows"))]
pub mod windows;

use self::types::*;
use crate::{
    ptr::{Array, WasmPtr},
    state::{
        get_stat_for_kind, host_file_type_to_wasi_file_type, Fd, InodeVal, Kind, WasiFile,
        WasiState, MAX_SYMLINKS,
    },
    ExitCode,
};
use rand::{thread_rng, Rng};
use std::cell::Cell;
use std::convert::Infallible;
use std::io::{self, Read, Seek, Write};
use wasmer_runtime_core::{debug, memory::Memory, vm::Ctx};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::*;

#[cfg(any(target_os = "windows"))]
pub use windows::*;

#[allow(clippy::mut_from_ref)]
fn get_wasi_state(ctx: &Ctx) -> &mut WasiState {
    unsafe { &mut *(ctx.data as *mut WasiState) }
}

fn write_bytes<T: Write>(
    mut write_loc: T,
    memory: &Memory,
    iovs_arr_cell: &[Cell<__wasi_ciovec_t>],
) -> Result<u32, __wasi_errno_t> {
    let mut bytes_written = 0;
    for iov in iovs_arr_cell {
        let iov_inner = iov.get();
        let bytes = iov_inner.buf.deref(memory, 0, iov_inner.buf_len)?;
        write_loc
            .write(&bytes.iter().map(|b_cell| b_cell.get()).collect::<Vec<u8>>())
            .map_err(|_| {
                write_loc.flush();
                __WASI_EIO
            })?;

        // TODO: handle failure more accurately
        bytes_written += iov_inner.buf_len;
    }
    write_loc.flush();
    Ok(bytes_written)
}

/// checks that `rights_check_set` is a subset of `rights_set`
fn has_rights(rights_set: __wasi_rights_t, rights_check_set: __wasi_rights_t) -> bool {
    rights_set | rights_check_set == rights_set
}

#[must_use]
fn write_buffer_array(
    memory: &Memory,
    from: &[Vec<u8>],
    ptr_buffer: WasmPtr<WasmPtr<u8, Array>, Array>,
    buffer: WasmPtr<u8, Array>,
) -> __wasi_errno_t {
    let ptrs = wasi_try!(ptr_buffer.deref(memory, 0, from.len() as u32));

    let mut current_buffer_offset = 0;
    for ((i, sub_buffer), ptr) in from.iter().enumerate().zip(ptrs.iter()) {
        ptr.set(WasmPtr::new(buffer.offset() + current_buffer_offset));

        let cells =
            wasi_try!(buffer.deref(memory, current_buffer_offset, sub_buffer.len() as u32 + 1));

        for (cell, &byte) in cells.iter().zip(sub_buffer.iter().chain([0].iter())) {
            cell.set(byte);
        }
        current_buffer_offset += sub_buffer.len() as u32 + 1;
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
    debug!("wasi::args_get");
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    let result = write_buffer_array(memory, &*state.args, argv, argv_buf);

    debug!(
        "=> args:\n{}",
        state
            .args
            .iter()
            .enumerate()
            .map(|(i, v)| format!(
                "{:>20}: {}",
                i,
                ::std::str::from_utf8(v).unwrap().to_string()
            ))
            .collect::<Vec<String>>()
            .join("\n")
    );

    result
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
    debug!("wasi::args_sizes_get");
    let memory = ctx.memory(0);

    let argc = wasi_try!(argc.deref(memory));
    let argv_buf_size = wasi_try!(argv_buf_size.deref(memory));

    let state = get_wasi_state(ctx);

    let argc_val = state.args.len() as u32;
    let argv_buf_size_val = state.args.iter().map(|v| v.len() as u32 + 1).sum();
    argc.set(argc_val);
    argv_buf_size.set(argv_buf_size_val);

    debug!("=> argc={}, argv_buf_size={}", argc_val, argv_buf_size_val);

    __WASI_ESUCCESS
}

/// ### `clock_res_get()`
/// Get the resolution of the specified clock
/// Input:
/// - `__wasi_clockid_t clock_id`
///     The ID of the clock to get the resolution of
/// Output:
/// - `__wasi_timestamp_t *resolution`
///     The resolution of the clock in nanoseconds
pub fn clock_res_get(
    ctx: &mut Ctx,
    clock_id: __wasi_clockid_t,
    resolution: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    debug!("wasi::clock_res_get");
    let memory = ctx.memory(0);

    let out_addr = wasi_try!(resolution.deref(memory));
    platform_clock_res_get(clock_id, out_addr)
}

/// ### `clock_time_get()`
/// Get the time of the specified clock
/// Inputs:
/// - `__wasi_clockid_t clock_id`
///     The ID of the clock to query
/// - `__wasi_timestamp_t precision`
///     The maximum amount of error the reading may have
/// Output:
/// - `__wasi_timestamp_t *time`
///     The value of the clock in nanoseconds
pub fn clock_time_get(
    ctx: &mut Ctx,
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    debug!(
        "wasi::clock_time_get clock_id: {}, precision: {}",
        clock_id, precision
    );
    let memory = ctx.memory(0);

    let out_addr = wasi_try!(time.deref(memory));
    let result = platform_clock_time_get(clock_id, precision, out_addr);
    debug!(
        "time: {} => {}",
        wasi_try!(time.deref(memory)).get(),
        result
    );
    result
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
    debug!("wasi::environ_get");
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    write_buffer_array(memory, &*state.envs, environ, environ_buf)
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
    debug!("wasi::environ_sizes_get");
    let memory = ctx.memory(0);

    let environ_count = wasi_try!(environ_count.deref(memory));
    let environ_buf_size = wasi_try!(environ_buf_size.deref(memory));

    let state = get_wasi_state(ctx);

    let env_var_count = state.envs.len() as u32;
    let env_buf_size = state.envs.iter().map(|v| v.len() as u32 + 1).sum();
    environ_count.set(env_var_count);
    environ_buf_size.set(env_buf_size);

    debug!(
        "env_var_count: {}, env_buf_size: {}",
        env_var_count, env_buf_size
    );

    __WASI_ESUCCESS
}

/// ### `fd_advise()`
/// Advise the system about how a file will be used
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor the advice applies to
/// - `__wasi_filesize_t offset`
///     The offset from which the advice applies
/// - `__wasi_filesize_t len`
///     The length from the offset to which the advice applies
/// - `__wasi_advice_t advice`
///     The advice to give
pub fn fd_advise(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
    advice: __wasi_advice_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_advise: fd={}", fd);

    // this is used for our own benefit, so just returning success is a valid
    // implementation for now
    __WASI_ESUCCESS
}

/// ### `fd_allocate`
/// Allocate extra space for a file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to allocate for
/// - `__wasi_filesize_t offset`
///     The offset from the start marking the beginning of the allocation
/// - `__wasi_filesize_t len`
///     The length from the offset marking the end of the allocation
pub fn fd_allocate(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_allocate");
    unimplemented!("wasi::fd_allocate")
}

/// ### `fd_close()`
/// Close an open file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     A file descriptor mapping to an open file to close
/// Errors:
/// - `__WASI_EISDIR`
///     If `fd` is a directory
/// - `__WASI_EBADF`
///     If `fd` is invalid or not open (TODO: consider __WASI_EINVAL)
pub fn fd_close(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_close");
    // FD is too large
    return __WASI_EMFILE;
    // FD is a directory (due to user input)
    return __WASI_EISDIR;
    // FD is invalid
    return __WASI_EBADF;
    __WASI_ESUCCESS
}

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to sync
pub fn fd_datasync(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_datasync");
    let state = get_wasi_state(ctx);

    if let Err(e) = state.fs.flush(fd) {
        e
    } else {
        __WASI_ESUCCESS
    }
}

/// ### `fd_fdstat_get()`
/// Get metadata of a file descriptor
/// Input:
/// - `__wasi_fd_t fd`
///     The file descriptor whose metadata will be accessed
/// Output:
/// - `__wasi_fdstat_t *buf`
///     The location where the metadata will be written
pub fn fd_fdstat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf_ptr: WasmPtr<__wasi_fdstat_t>,
) -> __wasi_errno_t {
    debug!(
        "wasi::fd_fdstat_get: fd={}, buf_ptr={}",
        fd,
        buf_ptr.offset()
    );
    let mut state = get_wasi_state(ctx);
    let memory = ctx.memory(0);
    let stat = wasi_try!(state.fs.fdstat(fd));
    let buf = wasi_try!(buf_ptr.deref(memory));

    buf.set(stat);

    __WASI_ESUCCESS
}

/// ### `fd_fdstat_set_flags()`
/// Set file descriptor flags for a file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to apply the new flags to
/// - `__wasi_fdflags_t flags`
///     The flags to apply to `fd`
pub fn fd_fdstat_set_flags(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    flags: __wasi_fdflags_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_fdstat_set_flags");
    let state = get_wasi_state(ctx);
    let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_FDSTAT_SET_FLAGS) {
        return __WASI_EACCES;
    }

    fd_entry.flags = flags;
    __WASI_ESUCCESS
}

/// ### `fd_fdstat_set_rights()`
/// Set the rights of a file descriptor.  This can only be used to remove rights
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to apply the new rights to
/// - `__wasi_rights_t fs_rights_base`
///     The rights to apply to `fd`
/// - `__wasi_rights_t fs_rights_inheriting`
///     The inheriting rights to apply to `fd`
pub fn fd_fdstat_set_rights(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_fdstat_set_rights");
    let state = get_wasi_state(ctx);
    let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

    // ensure new rights are a subset of current rights
    if fd_entry.rights | fs_rights_base != fd_entry.rights
        || fd_entry.rights_inheriting | fs_rights_inheriting != fd_entry.rights_inheriting
    {
        return __WASI_ENOTCAPABLE;
    }

    fd_entry.rights = fs_rights_base;
    fd_entry.rights_inheriting = fs_rights_inheriting;

    __WASI_ESUCCESS
}

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `__wasi_fd_t fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `__wasi_filestat_t *buf`
///     Where the metadata from `fd` will be written
pub fn fd_filestat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_filestat_t>,
) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_get");
    let mut state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    let stat = wasi_try!(state.fs.filestat_fd(fd));

    let buf = wasi_try!(buf.deref(memory));
    buf.set(stat);

    __WASI_ESUCCESS
}

pub fn fd_filestat_set_size(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    st_size: __wasi_filesize_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_set_size");
    unimplemented!("wasi::fd_filestat_set_size")
}

/// ### `fd_filestat_set_times()`
/// Set timestamp metadata on a file
/// Inputs:
/// - `__wasi_timestamp_t st_atim`
///     Last accessed time
/// - `__wasi_timestamp_t st_mtim`
///     Last modified time
/// - `__wasi_fstflags_t fst_flags`
///     Bit-vector for controlling which times get set
pub fn fd_filestat_set_times(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_set_times");
    let state = get_wasi_state(ctx);
    let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_FILESTAT_SET_TIMES) {
        return __WASI_EACCES;
    }

    if (fst_flags & __WASI_FILESTAT_SET_ATIM != 0 && fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0)
        || (fst_flags & __WASI_FILESTAT_SET_MTIM != 0
            && fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0)
    {
        return __WASI_EINVAL;
    }

    let inode = &mut state.fs.inodes[fd_entry.inode];

    if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 {
        inode.stat.st_atim = st_atim;
    } else if fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0 {
        // set to current real time
        unimplemented!("Set filestat time to the current real time");
    }

    if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 {
        inode.stat.st_mtim = st_mtim;
    } else if fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0 {
        // set to current real time
        unimplemented!("Set filestat time to the current real time");
    }

    __WASI_ESUCCESS
}

pub fn fd_pread(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t, Array>,
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nread: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::fd_pread");
    let memory = ctx.memory(0);

    let iov_cells = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nread_cell = wasi_try!(nread.deref(memory));

    unimplemented!("wasi::fd_pread");

    __WASI_ESUCCESS
}

/// ### `fd_prestat_get()`
/// Get metadata about a preopened file descriptor
/// Input:
/// - `__wasi_fd_t fd`
///     The preopened file descriptor to query
/// Output:
/// - `__wasi_prestat *buf`
///     Where the metadata will be written
pub fn fd_prestat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_prestat_t>,
) -> __wasi_errno_t {
    debug!("wasi::fd_prestat_get: fd={}", fd);
    let memory = ctx.memory(0);

    let prestat_ptr = wasi_try!(buf.deref(memory));

    let state = get_wasi_state(ctx);
    prestat_ptr.set(wasi_try!(state.fs.prestat_fd(fd)));

    __WASI_ESUCCESS
}

pub fn fd_prestat_dir_name(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!(
        "wasi::fd_prestat_dir_name: fd={}, path_len={}",
        fd, path_len
    );
    let memory = ctx.memory(0);
    let path_chars = wasi_try!(path.deref(memory, 0, path_len));

    let state = get_wasi_state(ctx);
    let real_fd = wasi_try!(state.fs.fd_map.get(&fd).ok_or(__WASI_EBADF));
    let inode_val = &state.fs.inodes[real_fd.inode];

    // check inode-val.is_preopened?

    if let Kind::Dir { .. } = inode_val.kind {
        // TODO: verify this: null termination, etc
        if inode_val.name.len() <= path_len as usize {
            let mut i = 0;
            for c in inode_val.name.bytes() {
                path_chars[i].set(c);
                i += 1
            }
            path_chars[i].set(0);

            debug!(
                "=> result: \"{}\"",
                ::std::str::from_utf8(unsafe { &*(&path_chars[..] as *const [_] as *const [u8]) })
                    .unwrap()
            );

            __WASI_ESUCCESS
        } else {
            __WASI_EOVERFLOW
        }
    } else {
        __WASI_ENOTDIR
    }
}

/// ### `fd_pwrite()`
/// Write to a file without adjusting its offset
/// Inputs:
/// - `__wasi_fd_t`
///     File descriptor (opened with writing) to write to
/// - `const __wasi_ciovec_t *iovs`
///     List of vectors to read data from
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// - `__wasi_filesize_t offset`
///     The offset to write at
/// Output:
/// - `u32 *nwritten`
///     Number of bytes written
pub fn fd_pwrite(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t, Array>,
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nwritten: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::fd_pwrite");
    // TODO: refactor, this is just copied from `fd_write`...
    let memory = ctx.memory(0);
    let iovs_arr_cell = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nwritten_cell = wasi_try!(nwritten.deref(memory));

    let bytes_written = match fd {
        0 => return __WASI_EINVAL,
        1 => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();

            wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
        }

        2 => {
            let stderr = io::stderr();
            let mut handle = stderr.lock();

            wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
        }
        _ => {
            let state = get_wasi_state(ctx);
            let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

            if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE) {
                // TODO: figure out the error to return when lacking rights
                return __WASI_EACCES;
            }

            let inode = &mut state.fs.inodes[fd_entry.inode];

            let bytes_written = match &mut inode.kind {
                Kind::File { handle } => {
                    handle.seek(::std::io::SeekFrom::Start(offset as u64));
                    wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
                }
                Kind::Dir { .. } => {
                    // TODO: verify
                    return __WASI_EISDIR;
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_pwrite"),
                Kind::Buffer { buffer } => wasi_try!(write_bytes(
                    &mut buffer[(offset as usize)..],
                    memory,
                    iovs_arr_cell
                )),
            };

            bytes_written
        }
    };

    nwritten_cell.set(bytes_written);

    __WASI_ESUCCESS
}

/// ### `fd_read()`
/// Read data from file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     File descriptor from which data will be read
/// - `const __wasi_iovec_t *iovs`
///     Vectors where data will be stored
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// Output:
/// - `u32 *nread`
///     Number of bytes read
pub fn fd_read(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t, Array>,
    iovs_len: u32,
    nread: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::fd_read: fd={}", fd);
    let memory = ctx.memory(0);

    let iovs_arr_cell = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nread_cell = wasi_try!(nread.deref(memory));

    fn read_bytes<T: Read>(
        mut reader: T,
        memory: &Memory,
        iovs_arr_cell: &[Cell<__wasi_iovec_t>],
    ) -> Result<u32, __wasi_errno_t> {
        let mut bytes_read = 0;

        for iov in iovs_arr_cell {
            let iov_inner = iov.get();
            let bytes = iov_inner.buf.deref(memory, 0, iov_inner.buf_len)?;
            let mut raw_bytes: &mut [u8] =
                unsafe { &mut *(bytes as *const [_] as *mut [_] as *mut [u8]) };
            bytes_read += reader.read(raw_bytes).map_err(|_| __WASI_EIO)? as u32;
        }
        Ok(bytes_read)
    }

    let bytes_read = match fd {
        0 => {
            let stdin = io::stdin();
            let mut handle = stdin.lock();

            wasi_try!(read_bytes(handle, memory, iovs_arr_cell))
        }
        1 | 2 => return __WASI_EINVAL,
        _ => {
            let state = get_wasi_state(ctx);
            let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

            if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_READ) {
                // TODO: figure out the error to return when lacking rights
                return __WASI_EACCES;
            }

            let offset = fd_entry.offset as usize;
            let inode = &mut state.fs.inodes[fd_entry.inode];

            let bytes_read = match &mut inode.kind {
                Kind::File { handle } => {
                    handle.seek(::std::io::SeekFrom::Start(offset as u64));
                    wasi_try!(read_bytes(handle, memory, iovs_arr_cell))
                }
                Kind::Dir { .. } => {
                    // TODO: verify
                    return __WASI_EISDIR;
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_read"),
                Kind::Buffer { buffer } => {
                    wasi_try!(read_bytes(&buffer[offset..], memory, iovs_arr_cell))
                }
            };

            fd_entry.offset += bytes_read as u64;

            bytes_read
        }
    };

    nread_cell.set(bytes_read);

    __WASI_ESUCCESS
}

/// ### `fd_readdir()`
/// Read data from directory specified by file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     File descriptor from which directory data will be read
/// - `void *buf`
///     Buffer where directory entries are stored
/// - `u32 buf_len`
///     Length of data in `buf`
/// - `__wasi_dircookie_t cookie`
///     Where the directory reading should start from
/// Output:
/// - `u32 *bufused`
///     The Number of bytes stored in `buf`; if less than `buf_len` then entire
///     directory has been read
pub fn fd_readdir(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    buf: WasmPtr<u8, Array>,
    buf_len: u32,
    cookie: __wasi_dircookie_t,
    bufused: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::fd_readdir");
    let memory = ctx.memory(0);
    let state = get_wasi_state(ctx);
    // TODO: figure out how this is supposed to work;
    // is it supposed to pack the buffer full every time until it can't? or do one at a time?

    let buf_arr_cell = wasi_try!(buf.deref(memory, 0, buf_len));
    let bufused_cell = wasi_try!(bufused.deref(memory));
    let working_dir = wasi_try!(state.fs.fd_map.get(&fd).ok_or(__WASI_EBADF));
    let mut cur_cookie = cookie;
    let mut buf_idx = 0;

    if let Kind::Dir { path, .. } = &state.fs.inodes[working_dir.inode].kind {
        // we need to support multiple calls,
        // simple and obviously correct implementation for now:
        // maintain consistent order via lexacographic sorting
        let mut entries = wasi_try!(wasi_try!(std::fs::read_dir(path).map_err(|_| __WASI_EIO))
            .collect::<Result<Vec<std::fs::DirEntry>, _>>()
            .map_err(|_| __WASI_EIO));
        entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        for entry in entries.iter().skip(cookie as usize) {
            cur_cookie += 1;
            let entry_path = entry.path();
            let entry_path = wasi_try!(entry_path.file_name().ok_or(__WASI_EIO));
            let entry_path_str = entry_path.to_string_lossy();
            let namlen = entry_path_str.len();
            debug!("Returning dirent for {}", entry_path_str);
            let dirent = __wasi_dirent_t {
                d_next: cur_cookie,
                d_ino: 0, // TODO: inode
                d_namlen: namlen as u32,
                d_type: host_file_type_to_wasi_file_type(wasi_try!(entry
                    .file_type()
                    .map_err(|_| __WASI_EIO))),
            };
            let dirent_bytes = dirent_to_le_bytes(&dirent);
            let upper_limit = std::cmp::min(
                buf_len as usize - buf_idx,
                std::mem::size_of::<__wasi_dirent_t>(),
            );
            for i in 0..upper_limit {
                buf_arr_cell[i + buf_idx].set(dirent_bytes[i]);
            }
            buf_idx += upper_limit;
            if upper_limit != std::mem::size_of::<__wasi_dirent_t>() {
                break;
            }
            let upper_limit = std::cmp::min(buf_len as usize - buf_idx, namlen);
            for (i, b) in entry_path_str.bytes().take(upper_limit).enumerate() {
                buf_arr_cell[i + buf_idx].set(b);
            }
            buf_idx += upper_limit;
            if upper_limit != namlen {
                break;
            }
        }
    } else {
        return __WASI_ENOTDIR;
    }
    bufused_cell.set(buf_idx as u32);
    __WASI_ESUCCESS
}

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `__wasi_fd_t from`
///     File descriptor to copy
/// - `__wasi_fd_t to`
///     Location to copy file descriptor to
pub fn fd_renumber(ctx: &mut Ctx, from: __wasi_fd_t, to: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_renumber: from={}, to={}", from, to);
    let state = get_wasi_state(ctx);
    let fd_entry = wasi_try!(state.fs.fd_map.get(&from).ok_or(__WASI_EBADF));
    let new_fd_entry = Fd {
        // TODO: verify this is correct
        rights: fd_entry.rights_inheriting,
        ..*fd_entry
    };

    state.fs.fd_map.insert(to, new_fd_entry);
    state.fs.fd_map.remove(&from);
    __WASI_ESUCCESS
}

/// ### `fd_seek()`
/// Update file descriptor offset
/// Inputs:
/// - `__wasi_fd_t fd`
///     File descriptor to mutate
/// - `__wasi_filedelta_t offset`
///     Number of bytes to adjust offset by
/// - `__wasi_whence_t whence`
///     What the offset is relative to
/// Output:
/// - `__wasi_filesize_t *fd`
///     The new offset relative to the start of the file
pub fn fd_seek(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: __wasi_filedelta_t,
    whence: __wasi_whence_t,
    newoffset: WasmPtr<__wasi_filesize_t>,
) -> __wasi_errno_t {
    debug!("wasi::fd_seek: fd={}, offset={}", fd, offset);
    let memory = ctx.memory(0);
    let state = get_wasi_state(ctx);
    let new_offset_cell = wasi_try!(newoffset.deref(memory));

    let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_SEEK) {
        return __WASI_EACCES;
    }

    // TODO: handle case if fd is a dir?
    match whence {
        __WASI_WHENCE_CUR => fd_entry.offset = (fd_entry.offset as i64 + offset) as u64,
        __WASI_WHENCE_END => {
            use std::io::SeekFrom;
            match state.fs.inodes[fd_entry.inode].kind {
                Kind::File { ref mut handle } => {
                    let end = wasi_try!(handle.seek(SeekFrom::End(0)).ok().ok_or(__WASI_EIO));
                    // TODO: handle case if fd_entry.offset uses 64 bits of a u64
                    fd_entry.offset = (end as i64 + offset) as u64;
                }
                Kind::Symlink { .. } => {
                    unimplemented!("wasi::fd_seek not implemented for symlinks")
                }
                Kind::Dir { .. } => {
                    // TODO: check this
                    return __WASI_EINVAL;
                }
                Kind::Buffer { .. } => {
                    // seeking buffers probably makes sense
                    // TODO: implement this
                    return __WASI_EINVAL;
                }
            }
        }
        __WASI_WHENCE_SET => fd_entry.offset = offset as u64,
        _ => return __WASI_EINVAL,
    }

    new_offset_cell.set(fd_entry.offset);

    __WASI_ESUCCESS
}

/// ### `fd_sync()`
/// Synchronize file and metadata to disk (TODO: expand upon what this means in our system)
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to sync
/// Errors:
/// TODO: figure out which errors this should return
/// - `__WASI_EPERM`
/// - `__WASI_ENOTCAPABLE`
pub fn fd_sync(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_sync");
    // TODO: check __WASI_RIGHT_FD_SYNC
    unimplemented!("wasi::fd_sync")
}

/// ### `fd_tell()`
/// Get the offset of the file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to access
/// Output:
/// - `__wasi_filesize_t *offset`
///     The offset of `fd` relative to the start of the file
pub fn fd_tell(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    offset: WasmPtr<__wasi_filesize_t>,
) -> __wasi_errno_t {
    debug!("wasi::fd_tell");
    let memory = ctx.memory(0);
    let state = get_wasi_state(ctx);
    let offset_cell = wasi_try!(offset.deref(memory));

    let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_TELL) {
        return __WASI_EACCES;
    }

    offset_cell.set(fd_entry.offset);

    __WASI_ESUCCESS
}

/// ### `fd_write()`
/// Write data to the file descriptor
/// Inputs:
/// - `__wasi_fd_t`
///     File descriptor (opened with writing) to write to
/// - `const __wasi_ciovec_t *iovs`
///     List of vectors to read data from
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// Output:
/// - `u32 *nwritten`
///     Number of bytes written
/// Errors:
///
pub fn fd_write(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t, Array>,
    iovs_len: u32,
    nwritten: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::fd_write: fd={}", fd);
    let memory = ctx.memory(0);
    let iovs_arr_cell = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nwritten_cell = wasi_try!(nwritten.deref(memory));

    let bytes_written = match fd {
        0 => return __WASI_EINVAL,
        1 => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();

            wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
        }

        2 => {
            let stderr = io::stderr();
            let mut handle = stderr.lock();

            wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
        }
        _ => {
            let state = get_wasi_state(ctx);
            let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

            if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE) {
                // TODO: figure out the error to return when lacking rights
                return __WASI_EACCES;
            }

            let offset = fd_entry.offset as usize;
            let inode = &mut state.fs.inodes[fd_entry.inode];

            let bytes_written = match &mut inode.kind {
                Kind::File { handle } => {
                    handle.seek(::std::io::SeekFrom::Start(offset as u64));

                    wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
                }
                Kind::Dir { .. } => {
                    // TODO: verify
                    return __WASI_EISDIR;
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_write"),
                Kind::Buffer { buffer } => {
                    wasi_try!(write_bytes(&mut buffer[offset..], memory, iovs_arr_cell))
                }
            };

            fd_entry.offset += bytes_written as u64;

            bytes_written
        }
    };

    nwritten_cell.set(bytes_written);

    __WASI_ESUCCESS
}

/// ### `path_create_directory()`
/// Create directory at a path
/// Inputs:
/// - `__wasi_fd_t fd`
///     The directory that the path is relative to
/// - `const char *path`
///     String containing path data
/// - `u32 path_len`
///     The length of `path`
/// Errors:
/// Required Rights:
/// - __WASI_RIGHT_PATH_CREATE_DIRECTORY
///     This right must be set on the directory that the file is created in (TODO: verify that this is true)
pub fn path_create_directory(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_create_directory");
    let memory = ctx.memory(0);
    let state = get_wasi_state(ctx);

    let working_dir = wasi_try!(state.fs.fd_map.get(&fd).ok_or(__WASI_EBADF));
    if !has_rights(working_dir.rights, __WASI_RIGHT_PATH_CREATE_DIRECTORY) {
        return __WASI_EACCES;
    }
    let path_cells = wasi_try!(path.deref(memory, 0, path_len));
    let path_string =
        wasi_try!(
            std::str::from_utf8(unsafe { &*(path_cells as *const [_] as *const [u8]) })
                .map_err(|_| __WASI_EINVAL)
        );
    debug!("=> path: {}", &path_string);

    let path = std::path::PathBuf::from(path_string);
    let path_vec = wasi_try!(path
        .components()
        .map(|comp| {
            comp.as_os_str()
                .to_str()
                .map(|inner_str| inner_str.to_string())
                .ok_or(__WASI_EINVAL)
        })
        .collect::<Result<Vec<String>, __wasi_errno_t>>());
    if path_vec.is_empty() {
        return __WASI_EINVAL;
    }

    assert!(
        path_vec.len() == 1,
        "path_create_directory for paths greater than depth 1 has not been implemented because our WASI FS abstractions are a work in progress.  We apologize for the inconvenience"
    );
    debug!("Path vec: {:#?}", path_vec);

    wasi_try!(std::fs::create_dir(&path).map_err(|_| __WASI_EIO));

    let kind = Kind::Dir {
        parent: Some(working_dir.inode),
        path: path.clone(),
        entries: Default::default(),
    };
    let new_inode = state.fs.inodes.insert(InodeVal {
        stat: wasi_try!(get_stat_for_kind(&kind).ok_or(__WASI_EIO)),
        is_preopened: false,
        name: path_vec[0].clone(),
        kind,
    });

    if let Kind::Dir { entries, .. } = &mut state.fs.inodes[working_dir.inode].kind {
        entries.insert(path_vec[0].clone(), new_inode);
    } else {
        return __WASI_ENOTDIR;
    }

    __WASI_ESUCCESS
}

/// ### `path_filestat_get()`
/// Access metadata about a file or directory
/// Inputs:
/// - `__wasi_fd_t fd`
///     The directory that `path` is relative to
/// - `__wasi_lookupflags_t flags`
///     Flags to control how `path` is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// Output:
/// - `__wasi_file_stat_t *buf`
///     The location where the metadata will be stored
pub fn path_filestat_get(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
    buf: WasmPtr<__wasi_filestat_t>,
) -> __wasi_errno_t {
    debug!("wasi::path_filestat_get");
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    let root_dir = wasi_try!(state.fs.fd_map.get(&fd).ok_or(__WASI_EBADF));

    if !has_rights(root_dir.rights, __WASI_RIGHT_PATH_FILESTAT_GET) {
        return __WASI_EACCES;
    }

    let path_string = wasi_try!(::std::str::from_utf8(unsafe {
        &*(wasi_try!(path.deref(memory, 0, path_len)) as *const [_] as *const [u8])
    })
    .map_err(|_| __WASI_EINVAL));
    debug!("=> path: {}", &path_string);
    let path = std::path::PathBuf::from(path_string);
    let path_vec = path
        .components()
        .map(|comp| comp.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<String>>();
    let buf_cell = wasi_try!(buf.deref(memory));

    if path_vec.is_empty() {
        return __WASI_EINVAL;
    }
    let mut cumulative_path = std::path::PathBuf::from(wasi_try!(state
        .fs
        .get_base_path_for_directory(root_dir.inode)
        .ok_or(__WASI_EIO)));

    debug!("=> Path vec: {:?}:", &path_vec);
    // find the inode by traversing the path
    let mut inode = root_dir.inode;
    'outer: for segment in &path_vec[..(path_vec.len() - 1)] {
        // loop to traverse symlinks
        // TODO: proper cycle detection
        let mut sym_count = 0;
        loop {
            match &state.fs.inodes[inode].kind {
                Kind::Dir { entries, .. } => {
                    cumulative_path.push(&segment);
                    if let Some(entry) = entries.get(segment) {
                        debug!("Entry {:?} found", &segment);
                        inode = entry.clone();
                    } else {
                        // lazily load
                        debug!("Lazily loading entry {:?}", &segment);
                        let path_metadata =
                            wasi_try!(cumulative_path.metadata().map_err(|_| __WASI_ENOENT));
                        if !path_metadata.is_dir() {
                            // TODO: should this just return invalid arg?
                            return __WASI_ENOTDIR;
                        }
                        let kind = Kind::Dir {
                            parent: Some(inode),
                            path: std::path::PathBuf::from(&segment),
                            entries: Default::default(),
                        };
                        let inode_val = InodeVal::from_file_metadata(
                            &path_metadata,
                            segment.clone(),
                            false,
                            kind,
                        );
                        let new_inode = state.fs.inodes.insert(inode_val);
                        let inode_idx = state.fs.inode_counter.get();
                        state.fs.inode_counter.replace(inode_idx + 1);
                        if let Kind::Dir { entries, .. } = &mut state.fs.inodes[inode].kind {
                            // check that we're not displacing any entries
                            assert!(entries.insert(segment.clone(), new_inode).is_none());
                            state.fs.inodes[new_inode].stat.st_ino = state.fs.inode_counter.get();
                            inode = new_inode;
                        }
                        debug!("Directory {:#?} lazily loaded", &cumulative_path);
                    }
                    continue 'outer;
                }
                Kind::Symlink { forwarded } => {
                    // TODO: updated cumulative path
                    sym_count += 1;
                    inode = forwarded.clone();
                    if sym_count > MAX_SYMLINKS {
                        return __WASI_ELOOP;
                    }
                }
                _ => {
                    return __WASI_ENOTDIR;
                }
            }
        }
    }

    let stat = match &state.fs.inodes[inode].kind {
        Kind::Dir { path, entries, .. } => {
            // read it from internal data structures if we can
            let last_segment = path_vec.last().unwrap();
            cumulative_path.push(last_segment);

            // read it from wasi FS cache first, otherwise check host system
            if entries.contains_key(last_segment) {
                state.fs.inodes[entries[last_segment]].stat
            } else {
                // otherwise read it from the host FS
                if !cumulative_path.exists() {
                    return __WASI_ENOENT;
                }
                let final_path_metadata =
                    wasi_try!(cumulative_path.metadata().map_err(|_| __WASI_EIO));
                let kind = if final_path_metadata.is_file() {
                    let file =
                        wasi_try!(std::fs::File::open(&cumulative_path).ok().ok_or(__WASI_EIO));
                    Kind::File {
                        handle: WasiFile::HostFile(file),
                    }
                } else if final_path_metadata.is_dir() {
                    Kind::Dir {
                        parent: Some(inode),
                        // TODO: verify that this doesn't cause issues with relative paths
                        path: cumulative_path.clone(),
                        entries: Default::default(),
                    }
                } else {
                    // TODO: check this
                    return __WASI_EINVAL;
                };

                wasi_try!(get_stat_for_kind(&kind).ok_or(__WASI_EIO))
            }
        }
        _ => {
            return __WASI_ENOTDIR;
        }
    };
    buf_cell.set(stat);

    __WASI_ESUCCESS
}

/// ### `path_filestat_set_times()`
/// Update time metadata on a file or directory
/// Inputs:
/// - `__wasi_fd_t fd`
///     The directory relative to which the path is resolved
/// - `__wasi_lookupflags_t flags`
///     Flags to control how the path is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// - `__wasi_timestamp_t st_atim`
///     The timestamp that the last accessed time attribute is set to
/// -  `__wasi_timestamp_t st_mtim`
///     The timestamp that the last modified time attribute is set to
/// - `__wasi_fstflags_t fst_flags`
///     A bitmask controlling which attributes are set
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
    debug!("wasi::path_filestat_set_times");
    unimplemented!("wasi::path_filestat_set_times")
}

/// ### `path_link()`
/// Create a hard link
/// Inputs:
/// - `__wasi_fd_t old_fd`
///     The directory relative to which the `old_path` is
/// - `__wasi_lookupflags_t old_flags`
///     Flags to control how `old_path` is understood
/// - `const char *old_path`
///     String containing the old file path
/// - `u32 old_path_len`
///     Length of the `old_path` string
/// - `__wasi_fd_t new_fd`
///     The directory relative to which the `new_path` is
/// - `const char *new_path`
///     String containing the new file path
/// - `u32 old_path_len`
///     Length of the `new_path` string
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
    debug!("wasi::path_link");
    unimplemented!("wasi::path_link")
}

/// ### `path_open()`
/// Open file located at the given path
/// Inputs:
/// - `__wasi_fd_t dirfd`
///     The fd corresponding to the directory that the file is in
/// - `__wasi_lookupflags_t dirflags`
///     Flags specifying how the path will be resolved
/// - `char *path`
///     The path of the file or directory to open
/// - `u32 path_len`
///     The length of the `path` string
/// - `__wasi_oflags_t o_flags`
///     How the file will be opened
/// - `__wasi_rights_t fs_rights_base`
///     The rights of the created file descriptor
/// - `__wasi_rights_t fs_rightsinheriting`
///     The rights of file descriptors derived from the created file descriptor
/// - `__wasi_fdflags_t fs_flags`
///     The flags of the file descriptor
/// Output:
/// - `__wasi_fd_t* fd`
///     The new file descriptor
/// Possible Errors:
/// - `__WASI_EACCES`, `__WASI_EBADF`, `__WASI_EFAULT`, `__WASI_EFBIG?`, `__WASI_EINVAL`, `__WASI_EIO`, `__WASI_ELOOP`, `__WASI_EMFILE`, `__WASI_ENAMETOOLONG?`, `__WASI_ENFILE`, `__WASI_ENOENT`, `__WASI_ENOTDIR`, `__WASI_EROFS`, and `__WASI_ENOTCAPABLE`
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
    debug!("wasi::path_open");
    let memory = ctx.memory(0);
    /* TODO: find actual upper bound on name size (also this is a path, not a name :think-fish:) */
    if path_len > 1024 * 1024 {
        return __WASI_ENAMETOOLONG;
    }

    let fd_cell = wasi_try!(fd.deref(memory));
    let path_cells = wasi_try!(path.deref(memory, 0, path_len));
    let state = get_wasi_state(ctx);

    // o_flags:
    // - __WASI_O_FLAG_CREAT (create if it does not exist)
    // - __WASI_O_DIRECTORY (fail if not dir)
    // - __WASI_O_EXCL (fail if file exists)
    // - __WASI_O_TRUNC (truncate size to 0)

    let working_dir = wasi_try!(state.fs.fd_map.get(&dirfd).ok_or(__WASI_EBADF));

    // ASSUMPTION: open rights apply recursively
    if !has_rights(working_dir.rights, __WASI_RIGHT_PATH_OPEN) {
        return __WASI_EACCES;
    }

    let path_string =
        wasi_try!(
            std::str::from_utf8(unsafe { &*(path_cells as *const [_] as *const [u8]) })
                .map_err(|_| __WASI_EINVAL)
        );
    let path = std::path::PathBuf::from(path_string);
    let path_vec = wasi_try!(path
        .components()
        .map(|comp| {
            comp.as_os_str()
                .to_str()
                .map(|inner_str| inner_str.to_string())
                .ok_or(__WASI_EINVAL)
        })
        .collect::<Result<Vec<String>, __wasi_errno_t>>());
    debug!("Path vec: {:#?}", path_vec);

    if path_vec.is_empty() {
        return __WASI_EINVAL;
    }

    let mut cur_dir_inode = working_dir.inode;
    let mut cumulative_path = std::path::PathBuf::from(wasi_try!(state
        .fs
        .get_base_path_for_directory(working_dir.inode)
        .ok_or(__WASI_EIO)));

    // traverse path
    if path_vec.len() > 1 {
        for path_segment in &path_vec[..(path_vec.len() - 1)] {
            match &state.fs.inodes[cur_dir_inode].kind {
                Kind::Dir { entries, .. } => {
                    if let Some(child) = entries.get(path_segment) {
                        cumulative_path.push(path_segment);
                        let inode_val = *child;
                        cur_dir_inode = inode_val;
                    } else {
                        // attempt to lazily load or create
                        if path_segment == ".." {
                            unimplemented!(
                                "\"..\" in paths in `path_open` has not been implemented yet"
                            );
                        }
                        // lazily load
                        cumulative_path.push(path_segment);
                        let mut open_options = std::fs::OpenOptions::new();
                        let open_options = open_options.read(true);
                        // ASSUMPTION: __WASI_O_CREAT applies recursively
                        let open_options = if o_flags & __WASI_O_CREAT != 0 {
                            open_options.create(true)
                        } else {
                            open_options
                        };
                        // TODO: handle __WASI_O_TRUNC on directories

                        // TODO: refactor and reuse
                        let cur_file_metadata =
                            wasi_try!(cumulative_path.metadata().map_err(|_| __WASI_EINVAL));
                        let kind = if cur_file_metadata.is_dir() {
                            Kind::Dir {
                                parent: Some(cur_dir_inode),
                                path: cumulative_path.clone(),
                                entries: Default::default(),
                            }
                        } else {
                            return __WASI_ENOTDIR;
                        };
                        let inode_val = InodeVal::from_file_metadata(
                            &cur_file_metadata,
                            path_segment.clone(),
                            false,
                            kind,
                        );

                        let new_inode = state.fs.inodes.insert(inode_val);
                        let inode_idx = state.fs.inode_counter.get();
                        state.fs.inode_counter.replace(inode_idx + 1);
                        // reborrow to insert entry
                        if let Kind::Dir { entries, .. } = &mut state.fs.inodes[cur_dir_inode].kind
                        {
                            assert!(entries.insert(path_segment.clone(), new_inode).is_none());
                            state.fs.inodes[new_inode].stat.st_ino = state.fs.inode_counter.get();
                            cur_dir_inode = new_inode;
                        }
                    }
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks not yet supported in `path_open`"),
                _ => return __WASI_ENOTDIR,
            }
        }
    }

    let file_name = path_vec.last().unwrap();

    debug!(
        "Looking for file {} in directory {:#?}",
        file_name, cumulative_path
    );

    cumulative_path.push(file_name);
    let file_path = cumulative_path;

    let out_fd = if let Kind::Dir {
        entries, parent, ..
    } = &mut state.fs.inodes[cur_dir_inode].kind
    {
        // short circuit logic if attempting to get parent
        if file_name == ".." {
            if let Some(p) = parent {
                let parent_inode = *p;
                wasi_try!(state.fs.create_fd(
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                    parent_inode
                ))
            } else {
                return __WASI_EACCES;
            }
        } else {
            if let Some(child) = entries.get(file_name).cloned() {
                let child_inode_val = &state.fs.inodes[child];
                // early return based on flags
                if o_flags & __WASI_O_EXCL != 0 {
                    return __WASI_EEXIST;
                }
                if o_flags & __WASI_O_DIRECTORY != 0 {
                    match &child_inode_val.kind {
                        Kind::Dir { .. } => (),
                        Kind::Symlink { .. } => {
                            unimplemented!("Symlinks not yet supported in path_open")
                        }
                        _ => return __WASI_ENOTDIR,
                    }
                }
                // do logic on child
                wasi_try!(state
                    .fs
                    .create_fd(fs_rights_base, fs_rights_inheriting, fs_flags, child))
            } else {
                debug!("Attempting to load file from host system");

                let file_metadata = file_path.metadata();
                // if entry does not exist in parent directory, try to lazily
                // load it; possibly creating or truncating it if flags set
                let kind = if file_metadata.is_ok() && file_metadata.unwrap().is_dir() {
                    // special dir logic
                    Kind::Dir {
                        parent: Some(cur_dir_inode),
                        path: file_path.clone(),
                        entries: Default::default(),
                    }
                } else {
                    // file is not a dir
                    let real_opened_file = {
                        let mut open_options = std::fs::OpenOptions::new();
                        let open_options = open_options.read(true);
                        let open_options = if fs_rights_base & __WASI_RIGHT_FD_WRITE != 0 {
                            open_options.write(true)
                        } else {
                            open_options
                        };
                        let open_options = if o_flags & __WASI_O_CREAT != 0 {
                            debug!(
                                "File {:?} may be created when opened if it does not exist",
                                &file_path
                            );
                            open_options.create(true)
                        } else {
                            open_options
                        };
                        let open_options = if o_flags & __WASI_O_TRUNC != 0 {
                            debug!("File {:?} will be truncated when opened", &file_path);
                            open_options.truncate(true)
                        } else {
                            open_options
                        };
                        debug!("Opening host file {:?}", &file_path);
                        let real_open_file =
                            wasi_try!(open_options.open(&file_path).map_err(|_| __WASI_EIO));

                        real_open_file
                    };
                    Kind::File {
                        handle: WasiFile::HostFile(real_opened_file),
                    }
                };

                // record lazily loaded or newly created fd
                let new_inode = state.fs.inodes.insert(InodeVal {
                    stat: wasi_try!(get_stat_for_kind(&kind).ok_or(__WASI_EIO)),
                    is_preopened: false,
                    name: file_name.clone(),
                    kind,
                });

                // reborrow to insert entry
                if let Kind::Dir { entries, .. } = &mut state.fs.inodes[working_dir.inode].kind {
                    entries.insert(file_name.clone(), new_inode);
                }
                let new_fd = wasi_try!(state.fs.create_fd(
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                    new_inode,
                ));

                new_fd
            }
        }
    } else {
        // working_dir did not match on Kind::Dir
        return __WASI_ENOTDIR;
    };

    fd_cell.set(out_fd);

    __WASI_ESUCCESS
}

pub fn path_readlink(
    ctx: &mut Ctx,
    dir_fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
    buf: WasmPtr<u8, Array>,
    buf_len: u32,
    buf_used: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::path_readlink");
    unimplemented!("wasi::path_readlink")
}

pub fn path_remove_directory(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_remove_directory");
    unimplemented!("wasi::path_remove_directory")
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
    debug!("wasi::path_rename");
    unimplemented!("wasi::path_rename")
}
pub fn path_symlink(
    ctx: &mut Ctx,
    old_path: WasmPtr<u8, Array>,
    old_path_len: u32,
    fd: __wasi_fd_t,
    new_path: WasmPtr<u8, Array>,
    new_path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_symlink");
    unimplemented!("wasi::path_symlink")
}
pub fn path_unlink_file(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_unlink_file");
    unimplemented!("wasi::path_unlink_file")
}
pub fn poll_oneoff(
    ctx: &mut Ctx,
    in_: WasmPtr<__wasi_subscription_t, Array>,
    out_: WasmPtr<__wasi_event_t, Array>,
    nsubscriptions: u32,
    nevents: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::poll_oneoff");
    unimplemented!("wasi::poll_oneoff")
}
pub fn proc_exit(ctx: &mut Ctx, code: __wasi_exitcode_t) -> Result<Infallible, ExitCode> {
    debug!("wasi::proc_exit, {}", code);
    Err(ExitCode { code })
}
pub fn proc_raise(ctx: &mut Ctx, sig: __wasi_signal_t) -> __wasi_errno_t {
    debug!("wasi::proc_raise");
    unimplemented!("wasi::proc_raise")
}

/// ### `random_get()`
/// Fill buffer with high-quality random data.  This function may be slow and block
/// Inputs:
/// - `void *buf`
///     A pointer to a buffer where the random bytes will be written
/// - `size_t buf_len`
///     The number of bytes that will be written
pub fn random_get(ctx: &mut Ctx, buf: WasmPtr<u8, Array>, buf_len: u32) -> __wasi_errno_t {
    debug!("wasi::random_get buf_len: {}", buf_len);
    let mut rng = thread_rng();
    let memory = ctx.memory(0);

    let buf = wasi_try!(buf.deref(memory, 0, buf_len));

    unsafe {
        let u8_buffer = &mut *(buf as *const [_] as *mut [_] as *mut [u8]);
        thread_rng().fill(u8_buffer);
    }

    __WASI_ESUCCESS
}

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(ctx: &mut Ctx) -> __wasi_errno_t {
    debug!("wasi::sched_yield");
    ::std::thread::yield_now();
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
    debug!("wasi::sock_recv");
    unimplemented!("wasi::sock_recv")
}
pub fn sock_send(
    ctx: &mut Ctx,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t, Array>,
    si_data_len: u32,
    si_flags: __wasi_siflags_t,
    so_datalen: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::sock_send");
    unimplemented!("wasi::sock_send")
}
pub fn sock_shutdown(ctx: &mut Ctx, sock: __wasi_fd_t, how: __wasi_sdflags_t) -> __wasi_errno_t {
    debug!("wasi::sock_shutdown");
    unimplemented!("wasi::sock_shutdown")
}
