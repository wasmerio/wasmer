#![allow(unused)]
pub mod types;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix;
#[cfg(any(target_os = "windows"))]
pub mod windows;

use self::types::*;
use crate::{
    ptr::{Array, WasmPtr},
    state::{Fd, Kind, WasiState, MAX_SYMLINKS},
};
use rand::{thread_rng, Rng};
use std::cell::Cell;
use std::io::{self, Read, Write};
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
            .map_err(|_| __WASI_EIO)?;

        // TODO: handle failure more accurately
        bytes_written += iov_inner.buf_len;
    }
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
    debug!("wasi::clock_time_get");
    let memory = ctx.memory(0);

    let out_addr = wasi_try!(time.deref(memory));
    platform_clock_time_get(clock_id, precision, out_addr)
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
    debug!("wasi::environ_sizes_get");
    let memory = ctx.memory(0);

    let environ_count = wasi_try!(environ_count.deref(memory));
    let environ_buf_size = wasi_try!(environ_buf_size.deref(memory));

    let state = get_wasi_state(ctx);

    environ_count.set(state.envs.len() as u32);
    environ_buf_size.set(state.envs.iter().map(|v| v.len() as u32).sum());

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
    unimplemented!()
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
    buf: WasmPtr<__wasi_fdstat_t>,
) -> __wasi_errno_t {
    debug!("wasi::fd_fdstat_get: fd={}", fd);
    let mut state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    let stat = wasi_try!(state.fs.fdstat(fd));

    let buf = wasi_try!(buf.deref(memory));
    buf.set(stat);

    __WASI_EFAULT
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
    unimplemented!()
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
        unimplemented!();
    }

    if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 {
        inode.stat.st_mtim = st_mtim;
    } else if fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0 {
        // set to current real time
        unimplemented!();
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

    unimplemented!();

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

    if let Ok(path_chars) = path.deref(memory, 0, path_len) {
        debug!(
            "=> path: {}",
            path_chars
                .iter()
                .map(|c| c.get() as char)
                .collect::<String>()
        );
        if true
        /* check if dir */
        {
            // get name
            // write name
            // if overflow __WASI_EOVERFLOW
            __WASI_ESUCCESS
        } else {
            __WASI_ENOTDIR
        }
    } else {
        __WASI_EFAULT
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
                    // TODO: adjust by offset
                    wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
                }
                Kind::Dir { .. } => {
                    // TODO: verify
                    return __WASI_EISDIR;
                }
                Kind::Symlink { .. } => unimplemented!(),
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
                Kind::File { handle } => wasi_try!(read_bytes(handle, memory, iovs_arr_cell)),
                Kind::Dir { .. } => {
                    // TODO: verify
                    return __WASI_EISDIR;
                }
                Kind::Symlink { .. } => unimplemented!(),
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

    if let (Ok(buf_arr_cell), Ok(bufused_cell)) =
        (buf.deref(memory, 0, buf_len), bufused.deref(memory))
    {
        unimplemented!()
    } else {
        __WASI_EFAULT
    }
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

    state.fs.fd_map.insert(
        to,
        Fd {
            // TODO: verify this is correct
            rights: fd_entry.rights_inheriting,
            ..*fd_entry
        },
    );
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
    debug!("wasi::fd_seek: fd={}", fd);
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
        __WASI_WHENCE_END => unimplemented!(),
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
/// - `__WAIS_ENOTCAPABLE`
pub fn fd_sync(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_sync");
    // TODO: check __WASI_RIGHT_FD_SYNC
    unimplemented!()
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
                Kind::File { handle } => wasi_try!(write_bytes(handle, memory, iovs_arr_cell)),
                Kind::Dir { .. } => {
                    // TODO: verify
                    return __WASI_EISDIR;
                }
                Kind::Symlink { .. } => unimplemented!(),
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
    // check __WASI_RIGHT_PATH_CREATE_DIRECTORY
    unimplemented!()
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

    let path_vec = wasi_try!(::std::str::from_utf8(unsafe {
        &*(wasi_try!(path.deref(memory, 0, path_len)) as *const [_] as *const [u8])
    })
    .map_err(|_| __WASI_EINVAL))
    .split('/')
    .map(|str| str.to_string())
    .collect::<Vec<String>>();
    let buf_cell = wasi_try!(buf.deref(memory));

    // find the inode by traversing the path
    let mut inode = root_dir.inode;
    'outer: for segment in path_vec {
        // loop to traverse symlinks
        // TODO: proper cycle detection
        let mut sym_count = 0;
        loop {
            match &state.fs.inodes[inode].kind {
                Kind::Dir { entries, .. } => {
                    if let Some(entry) = entries.get(&segment) {
                        inode = entry.clone();
                        continue 'outer;
                    } else {
                        return __WASI_ENOENT;
                    }
                }
                Kind::Symlink { forwarded } => {
                    sym_count += 1;
                    inode = forwarded.clone();
                    if sym_count > MAX_SYMLINKS {
                        return __WASI_ELOOP;
                    }
                }
                _ => return __WASI_ENOTDIR,
            }
        }
    }

    let stat = state.fs.inodes[inode].stat;

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
    unimplemented!()
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
    unimplemented!()
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
    if path_len > 1024
    /* TODO: find actual upper bound on name size (also this is a path, not a name :think-fish:) */
    {
        return __WASI_ENAMETOOLONG;
    }

    // check for __WASI_RIGHT_PATH_OPEN somewhere, probably via dirfd
    let fd_cell = wasi_try!(fd.deref(memory));
    let path_cells = wasi_try!(path.deref(memory, 0, path_len));
    let state = get_wasi_state(ctx);

    // o_flags:
    // - __WASI_O_FLAG_CREAT (create if it does not exist)
    // - __WASI_O_DIRECTORY (fail if not dir)
    // - __WASI_O_EXCL (fail if file exists)
    // - __WASI_O_TRUNC (truncate size to 0)

    let working_dir = wasi_try!(state.fs.fd_map.get(&dirfd).ok_or(__WASI_EBADF));

    // ASSUMPTION: open rights cascade down
    if !has_rights(working_dir.rights, __WASI_RIGHT_PATH_OPEN) {
        return __WASI_EACCES;
    }

    let path_vec =
        wasi_try!(
            ::std::str::from_utf8(unsafe { &*(path_cells as *const [_] as *const [u8]) })
                .map_err(|_| __WASI_EINVAL)
        )
        .split('/')
        .map(|str| str.to_string())
        .collect::<Vec<String>>();

    if path_vec.is_empty() {
        return __WASI_EINVAL;
    }

    let working_dir_inode = &mut state.fs.inodes[working_dir.inode];

    let mut cur_dir = working_dir;

    // TODO: refactor single path segment logic out and do traversing before
    // as necessary
    let out_fd = if path_vec.len() == 1 {
        // just the file or dir
        if let Kind::Dir { entries, .. } = &mut working_dir_inode.kind {
            if let Some(child) = entries.get(&path_vec[0]).cloned() {
                let child_inode_val = &state.fs.inodes[child];
                // early return based on flags
                if o_flags & __WASI_O_EXCL != 0 {
                    return __WASI_EEXIST;
                }
                if o_flags & __WASI_O_DIRECTORY != 0 {
                    match &child_inode_val.kind {
                        Kind::Dir { .. } => (),
                        Kind::Symlink { .. } => unimplemented!(),
                        _ => return __WASI_ENOTDIR,
                    }
                }
                // do logic on child
                wasi_try!(state
                    .fs
                    .create_fd(fs_rights_base, fs_rights_inheriting, fs_flags, child))
            } else {
                // entry does not exist in parent directory
                // check to see if we should create it
                if o_flags & __WASI_O_CREAT != 0 {
                    // insert in to directory and set values
                    //entries.insert(path_segment[0], )
                    unimplemented!()
                } else {
                    // no entry and can't create it
                    return __WASI_ENOENT;
                }
            }
        } else {
            // working_dir is not a directory
            return __WASI_ENOTDIR;
        }
    } else {
        // traverse the pieces of the path
        // TODO: lots of testing on this
        for path_segment in &path_vec[..(path_vec.len() - 1)] {
            match &working_dir_inode.kind {
                Kind::Dir { entries, .. } => {
                    if let Some(child) = entries.get(path_segment) {
                        unimplemented!();
                    } else {
                        // Is __WASI_O_FLAG_CREAT recursive?
                        // ASSUMPTION: it's not
                        return __WASI_EINVAL;
                    }
                }
                Kind::Symlink { .. } => unimplemented!(),
                _ => return __WASI_ENOTDIR,
            }
        }
        unimplemented!()
    };

    fd_cell.set(out_fd);

    __WASI_ESUCCESS
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
    debug!("wasi::path_readlink");
    unimplemented!()
}
pub fn path_remove_directory(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_remove_directory");
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
    debug!("wasi::path_rename");
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
    debug!("wasi::path_symlink");
    unimplemented!()
}
pub fn path_unlink_file(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_unlink_file");
    unimplemented!()
}
pub fn poll_oneoff(
    ctx: &mut Ctx,
    in_: WasmPtr<__wasi_subscription_t, Array>,
    out_: WasmPtr<__wasi_event_t, Array>,
    nsubscriptions: u32,
    nevents: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::poll_oneoff");
    unimplemented!()
}
pub fn proc_exit(ctx: &mut Ctx, rval: __wasi_exitcode_t) -> Result<(), &'static str> {
    debug!("wasi::proc_exit, {}", rval);
    Err("Instance exited")
}
pub fn proc_raise(ctx: &mut Ctx, sig: __wasi_signal_t) -> __wasi_errno_t {
    debug!("wasi::proc_raise");
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
    debug!("wasi::random_get");
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
    debug!("wasi::sock_send");
    unimplemented!()
}
pub fn sock_shutdown(ctx: &mut Ctx, sock: __wasi_fd_t, how: __wasi_sdflags_t) -> __wasi_errno_t {
    debug!("wasi::sock_shutdown");
    unimplemented!()
}
