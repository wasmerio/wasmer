#![allow(unused)]
pub mod types;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix;
#[cfg(any(target_os = "windows"))]
pub mod windows;

pub mod legacy;

use self::types::*;
use crate::{
    ptr::{Array, WasmPtr},
    state::{
        self, host_file_type_to_wasi_file_type, iterate_poll_events, poll, Fd, HostFile, Inode,
        InodeVal, Kind, PollEvent, PollEventBuilder, WasiFile, WasiFsError, WasiState,
        MAX_SYMLINKS,
    },
    ExitCode,
};
use std::borrow::Borrow;
use std::cell::Cell;
use std::convert::{Infallible, TryInto};
use std::io::{self, Read, Seek, Write};
use wasmer_runtime_core::{debug, memory::Memory, vm::Ctx};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::*;

#[cfg(any(target_os = "windows"))]
pub use windows::*;

/// This function is not safe
#[allow(clippy::mut_from_ref)]
pub(crate) fn get_memory_and_wasi_state(
    ctx: &mut Ctx,
    mem_index: u32,
) -> (&Memory, &mut WasiState) {
    unsafe { ctx.memory_and_data_mut(mem_index) }
}

fn write_bytes_inner<T: Write>(
    mut write_loc: T,
    memory: &Memory,
    iovs_arr_cell: &[Cell<__wasi_ciovec_t>],
) -> Result<u32, __wasi_errno_t> {
    let mut bytes_written = 0;
    for iov in iovs_arr_cell {
        let iov_inner = iov.get();
        let bytes = iov_inner.buf.deref(memory, 0, iov_inner.buf_len)?;
        write_loc
            .write_all(&bytes.iter().map(|b_cell| b_cell.get()).collect::<Vec<u8>>())
            .map_err(|_| __WASI_EIO)?;

        // TODO: handle failure more accurately
        bytes_written += iov_inner.buf_len;
    }
    Ok(bytes_written)
}

fn write_bytes<T: Write>(
    mut write_loc: T,
    memory: &Memory,
    iovs_arr_cell: &[Cell<__wasi_ciovec_t>],
) -> Result<u32, __wasi_errno_t> {
    let result = write_bytes_inner(&mut write_loc, memory, iovs_arr_cell);
    write_loc.flush();
    result
}

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

fn get_current_time_in_nanos() -> Result<__wasi_timestamp_t, __wasi_errno_t> {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_err(|_| __WASI_EIO)?;
    Ok(duration.as_nanos() as __wasi_timestamp_t)
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let argc = wasi_try!(argc.deref(memory));
    let argv_buf_size = wasi_try!(argv_buf_size.deref(memory));

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let environ_count = wasi_try!(environ_count.deref(memory));
    let environ_buf_size = wasi_try!(environ_buf_size.deref(memory));

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd)).clone();
    let inode = fd_entry.inode;

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_ALLOCATE) {
        return __WASI_EACCES;
    }
    let new_size = wasi_try!(offset.checked_add(len), __WASI_EINVAL);

    match &mut state.fs.inodes[inode].kind {
        Kind::File { handle, .. } => {
            if let Some(handle) = handle {
                wasi_try!(handle.set_len(new_size).map_err(WasiFsError::into_wasi_err));
            } else {
                return __WASI_EBADF;
            }
        }
        Kind::Buffer { buffer } => {
            buffer.resize(new_size as usize, 0);
        }
        Kind::Symlink { .. } => return __WASI_EBADF,
        Kind::Dir { .. } | Kind::Root { .. } => return __WASI_EISDIR,
    }
    state.fs.inodes[inode].stat.st_size = new_size;
    debug!("New file size: {}", new_size);

    __WASI_ESUCCESS
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
///     If `fd` is invalid or not open
pub fn fd_close(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_close");
    debug!("=> fd={}", fd);
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let fd_entry = wasi_try!(state.fs.get_fd(fd)).clone();

    wasi_try!(state.fs.close_fd(fd));

    __WASI_ESUCCESS
}

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to sync
pub fn fd_datasync(ctx: &mut Ctx, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_datasync");
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd)).clone();
    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_DATASYNC) {
        return __WASI_EACCES;
    }

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd)).clone();

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_FILESTAT_GET) {
        return __WASI_EACCES;
    }

    let stat = wasi_try!(state.fs.filestat_fd(fd));

    let buf = wasi_try!(buf.deref(memory));
    buf.set(stat);

    __WASI_ESUCCESS
}

/// ### `fd_filestat_set_size()`
/// Change the size of an open file, zeroing out any new bytes
/// Inputs:
/// - `__wasi_fd_t fd`
///     File descriptor to adjust
/// - `__wasi_filesize_t st_size`
///     New size that `fd` will be set to
pub fn fd_filestat_set_size(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    st_size: __wasi_filesize_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_set_size");
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd)).clone();
    let inode = fd_entry.inode;

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_FILESTAT_SET_SIZE) {
        return __WASI_EACCES;
    }

    match &mut state.fs.inodes[inode].kind {
        Kind::File { handle, .. } => {
            if let Some(handle) = handle {
                wasi_try!(handle.set_len(st_size).map_err(WasiFsError::into_wasi_err));
            } else {
                return __WASI_EBADF;
            }
        }
        Kind::Buffer { buffer } => {
            buffer.resize(st_size as usize, 0);
        }
        Kind::Symlink { .. } => return __WASI_EBADF,
        Kind::Dir { .. } | Kind::Root { .. } => return __WASI_EISDIR,
    }
    state.fs.inodes[inode].stat.st_size = st_size;

    __WASI_ESUCCESS
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
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

    if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 || fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.st_atim = time_to_set;
        // TODO: set it for more than just files
        match &mut inode.kind {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    handle.set_last_accessed(time_to_set);
                }
            }
            _ => {}
        }
    }

    if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 || fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.st_mtim = time_to_set;
        // TODO: set it for more than just files
        match &mut inode.kind {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    handle.set_last_modified(time_to_set);
                }
            }
            _ => {}
        }
    }

    __WASI_ESUCCESS
}

/// ### `fd_pread()`
/// Read from the file at the given offset without updating the file cursor.
/// This acts like a stateless version of Seek + Read
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to read the data with
/// - `const __wasi_iovec_t* iovs'
///     Vectors where the data will be stored
/// - `size_t iovs_len`
///     The number of vectors to store the data into
/// - `__wasi_filesize_t offset`
///     The file cursor to use: the starting position from which data will be read
/// Output:
/// - `size_t nread`
///     The number of bytes read
pub fn fd_pread(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t, Array>,
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nread: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::fd_pread");
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let iov_cells = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nread_cell = wasi_try!(nread.deref(memory));

    let bytes_read = match fd {
        __WASI_STDIN_FILENO => {
            if let Some(ref mut stdin) =
                wasi_try!(state.fs.stdin_mut().map_err(WasiFsError::into_wasi_err))
            {
                wasi_try!(read_bytes(stdin, memory, iov_cells))
            } else {
                return __WASI_EBADF;
            }
        }
        __WASI_STDOUT_FILENO => return __WASI_EINVAL,
        __WASI_STDERR_FILENO => return __WASI_EINVAL,
        _ => {
            let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));
            let inode = fd_entry.inode;

            if !(has_rights(fd_entry.rights, __WASI_RIGHT_FD_READ)
                && has_rights(fd_entry.rights, __WASI_RIGHT_FD_SEEK))
            {
                return __WASI_EACCES;
            }
            match &mut state.fs.inodes[inode].kind {
                Kind::File { handle, .. } => {
                    if let Some(h) = handle {
                        wasi_try!(
                            h.seek(std::io::SeekFrom::Start(offset as u64)).ok(),
                            __WASI_EIO
                        );
                        let bytes_read = wasi_try!(read_bytes(h, memory, iov_cells));
                        bytes_read
                    } else {
                        return __WASI_EINVAL;
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => return __WASI_EISDIR,
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_pread"),
                Kind::Buffer { buffer } => {
                    wasi_try!(read_bytes(&buffer[(offset as usize)..], memory, iov_cells))
                }
            }
        }
    };

    nread_cell.set(bytes_read);
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let prestat_ptr = wasi_try!(buf.deref(memory));

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let path_chars = wasi_try!(path.deref(memory, 0, path_len));

    let real_fd = wasi_try!(state.fs.fd_map.get(&fd).ok_or(__WASI_EBADF));
    let inode_val = &state.fs.inodes[real_fd.inode];

    // check inode-val.is_preopened?

    match inode_val.kind {
        Kind::Dir { .. } | Kind::Root { .. } => {
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
                    ::std::str::from_utf8(unsafe {
                        &*(&path_chars[..] as *const [_] as *const [u8])
                    })
                    .unwrap()
                );

                __WASI_ESUCCESS
            } else {
                __WASI_EOVERFLOW
            }
        }
        Kind::Symlink { .. } | Kind::Buffer { .. } | Kind::File { .. } => __WASI_ENOTDIR,
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let iovs_arr_cell = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nwritten_cell = wasi_try!(nwritten.deref(memory));

    let bytes_written = match fd {
        __WASI_STDIN_FILENO => return __WASI_EINVAL,
        __WASI_STDOUT_FILENO => {
            if let Some(ref mut stdout) =
                wasi_try!(state.fs.stdout_mut().map_err(WasiFsError::into_wasi_err))
            {
                wasi_try!(write_bytes(stdout, memory, iovs_arr_cell))
            } else {
                return __WASI_EBADF;
            }
        }
        __WASI_STDERR_FILENO => {
            if let Some(ref mut stderr) =
                wasi_try!(state.fs.stderr_mut().map_err(WasiFsError::into_wasi_err))
            {
                wasi_try!(write_bytes(stderr, memory, iovs_arr_cell))
            } else {
                return __WASI_EBADF;
            }
        }
        _ => {
            let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

            if !(has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE)
                && has_rights(fd_entry.rights, __WASI_RIGHT_FD_SEEK))
            {
                return __WASI_EACCES;
            }

            let inode = &mut state.fs.inodes[fd_entry.inode];

            let bytes_written = match &mut inode.kind {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        handle.seek(std::io::SeekFrom::Start(offset as u64));
                        wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
                    } else {
                        return __WASI_EINVAL;
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let iovs_arr_cell = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nread_cell = wasi_try!(nread.deref(memory));

    let bytes_read = match fd {
        __WASI_STDIN_FILENO => {
            if let Some(ref mut stdin) =
                wasi_try!(state.fs.stdin_mut().map_err(WasiFsError::into_wasi_err))
            {
                wasi_try!(read_bytes(stdin, memory, iovs_arr_cell))
            } else {
                return __WASI_EBADF;
            }
        }
        __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => return __WASI_EINVAL,
        _ => {
            let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

            if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_READ) {
                // TODO: figure out the error to return when lacking rights
                return __WASI_EACCES;
            }

            let offset = fd_entry.offset as usize;
            let inode = &mut state.fs.inodes[fd_entry.inode];

            let bytes_read = match &mut inode.kind {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        handle.seek(std::io::SeekFrom::Start(offset as u64));
                        wasi_try!(read_bytes(handle, memory, iovs_arr_cell))
                    } else {
                        return __WASI_EINVAL;
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    // TODO: figure out how this is supposed to work;
    // is it supposed to pack the buffer full every time until it can't? or do one at a time?

    let buf_arr_cell = wasi_try!(buf.deref(memory, 0, buf_len));
    let bufused_cell = wasi_try!(bufused.deref(memory));
    let working_dir = wasi_try!(state.fs.fd_map.get(&fd).ok_or(__WASI_EBADF));
    let mut cur_cookie = cookie;
    let mut buf_idx = 0;

    let entries = match &state.fs.inodes[working_dir.inode].kind {
        Kind::Dir { path, .. } => {
            // TODO: refactor this code
            // we need to support multiple calls,
            // simple and obviously correct implementation for now:
            // maintain consistent order via lexacographic sorting
            let mut entries = wasi_try!(wasi_try!(std::fs::read_dir(path).map_err(|_| __WASI_EIO))
                .collect::<Result<Vec<std::fs::DirEntry>, _>>()
                .map_err(|_| __WASI_EIO));
            entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            wasi_try!(entries
                .into_iter()
                .map(|entry| Ok((
                    entry.file_name().to_string_lossy().to_string(),
                    host_file_type_to_wasi_file_type(entry.file_type().map_err(|_| __WASI_EIO)?),
                    0, // TODO: inode
                )))
                .collect::<Result<Vec<(String, u8, u64)>, __wasi_errno_t>>())
        }
        Kind::Root { entries } => {
            let sorted_entries = {
                let mut entry_vec: Vec<(String, Inode)> =
                    entries.iter().map(|(a, b)| (a.clone(), *b)).collect();
                entry_vec.sort_by(|a, b| a.0.cmp(&b.0));
                entry_vec
            };
            sorted_entries
                .into_iter()
                .map(|(name, inode)| {
                    let entry = &state.fs.inodes[inode];
                    (
                        format!("/{}", entry.name),
                        entry.stat.st_filetype,
                        entry.stat.st_ino,
                    )
                })
                .collect()
        }
        Kind::File { .. } | Kind::Symlink { .. } | Kind::Buffer { .. } => return __WASI_ENOTDIR,
    };

    for (entry_path_str, wasi_file_type, ino) in entries.iter().skip(cookie as usize) {
        cur_cookie += 1;
        let namlen = entry_path_str.len();
        debug!("Returning dirent for {}", entry_path_str);
        let dirent = __wasi_dirent_t {
            d_next: cur_cookie,
            d_ino: *ino,
            d_namlen: namlen as u32,
            d_type: *wasi_file_type,
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
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
                Kind::File { ref mut handle, .. } => {
                    if let Some(handle) = handle {
                        let end = wasi_try!(handle.seek(SeekFrom::End(0)).ok().ok_or(__WASI_EIO));
                        // TODO: handle case if fd_entry.offset uses 64 bits of a u64
                        fd_entry.offset = (end as i64 + offset) as u64;
                    } else {
                        return __WASI_EINVAL;
                    }
                }
                Kind::Symlink { .. } => {
                    unimplemented!("wasi::fd_seek not implemented for symlinks")
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
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
    debug!("=> fd={}", fd);
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_SYNC) {
        return __WASI_EACCES;
    }
    let inode = fd_entry.inode;

    // TODO: implement this for more than files
    match &mut state.fs.inodes[inode].kind {
        Kind::File { handle, .. } => {
            if let Some(h) = handle {
                wasi_try!(h.sync_to_disk().map_err(WasiFsError::into_wasi_err));
            } else {
                return __WASI_EINVAL;
            }
        }
        Kind::Root { .. } | Kind::Dir { .. } => return __WASI_EISDIR,
        Kind::Buffer { .. } | Kind::Symlink { .. } => return __WASI_EINVAL,
    }

    __WASI_ESUCCESS
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let iovs_arr_cell = wasi_try!(iovs.deref(memory, 0, iovs_len));
    let nwritten_cell = wasi_try!(nwritten.deref(memory));

    let bytes_written = match fd {
        __WASI_STDIN_FILENO => return __WASI_EINVAL,
        __WASI_STDOUT_FILENO => {
            if let Some(ref mut stdout) =
                wasi_try!(state.fs.stdout_mut().map_err(WasiFsError::into_wasi_err))
            {
                wasi_try!(write_bytes(stdout, memory, iovs_arr_cell))
            } else {
                return __WASI_EBADF;
            }
        }
        __WASI_STDERR_FILENO => {
            if let Some(ref mut stderr) =
                wasi_try!(state.fs.stderr_mut().map_err(WasiFsError::into_wasi_err))
            {
                wasi_try!(write_bytes(stderr, memory, iovs_arr_cell))
            } else {
                return __WASI_EBADF;
            }
        }
        _ => {
            let fd_entry = wasi_try!(state.fs.fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

            if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE) {
                return __WASI_EACCES;
            }

            let offset = fd_entry.offset as usize;
            let inode = &mut state.fs.inodes[fd_entry.inode];

            let bytes_written = match &mut inode.kind {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        handle.seek(std::io::SeekFrom::Start(offset as u64));
                        wasi_try!(write_bytes(handle, memory, iovs_arr_cell))
                    } else {
                        return __WASI_EINVAL;
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return __WASI_EISDIR;
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_write"),
                Kind::Buffer { buffer } => {
                    wasi_try!(write_bytes(&mut buffer[offset..], memory, iovs_arr_cell))
                }
            };

            fd_entry.offset += bytes_written as u64;
            wasi_try!(state.fs.filestat_resync_size(fd));

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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let working_dir = wasi_try!(state.fs.get_fd(fd)).clone();
    if let Kind::Root { .. } = &state.fs.inodes[working_dir.inode].kind {
        return __WASI_EACCES;
    }
    if !has_rights(working_dir.rights, __WASI_RIGHT_PATH_CREATE_DIRECTORY) {
        return __WASI_EACCES;
    }
    let path_string = get_input_str!(memory, path, path_len);
    debug!("=> fd: {}, path: {}", fd, &path_string);

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

    debug!("Looking at components {:?}", &path_vec);

    let mut cur_dir_inode = working_dir.inode;
    for comp in &path_vec {
        debug!("Creating dir {}", comp);
        match &mut state.fs.inodes[cur_dir_inode].kind {
            Kind::Dir {
                ref mut entries,
                path,
                parent,
            } => {
                match comp.borrow() {
                    ".." => {
                        if let Some(p) = parent {
                            cur_dir_inode = *p;
                            continue;
                        }
                    }
                    "." => continue,
                    _ => (),
                }
                if let Some(child) = entries.get(comp) {
                    cur_dir_inode = *child;
                } else {
                    let mut adjusted_path = path.clone();
                    // TODO: double check this doesn't risk breaking the sandbox
                    adjusted_path.push(comp);
                    if adjusted_path.exists() && !adjusted_path.is_dir() {
                        return __WASI_ENOTDIR;
                    } else if !adjusted_path.exists() {
                        wasi_try!(std::fs::create_dir(&adjusted_path).ok(), __WASI_EIO);
                    }
                    let kind = Kind::Dir {
                        parent: Some(cur_dir_inode),
                        path: adjusted_path,
                        entries: Default::default(),
                    };
                    let new_inode = wasi_try!(state.fs.create_inode(kind, false, comp.to_string()));
                    // reborrow to insert
                    if let Kind::Dir {
                        ref mut entries, ..
                    } = &mut state.fs.inodes[cur_dir_inode].kind
                    {
                        entries.insert(comp.to_string(), new_inode);
                    }
                    cur_dir_inode = new_inode;
                }
            }
            Kind::Root { .. } => return __WASI_EACCES,
            _ => return __WASI_ENOTDIR,
        }
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let root_dir = wasi_try!(state.fs.get_fd(fd));

    if !has_rights(root_dir.rights, __WASI_RIGHT_PATH_FILESTAT_GET) {
        return __WASI_EACCES;
    }
    let path_string = get_input_str!(memory, path, path_len);

    debug!("=> base_fd: {}, path: {}", fd, &path_string);

    let file_inode = wasi_try!(state.fs.get_inode_at_path(
        fd,
        path_string,
        flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    ));
    let stat = wasi_try!(state
        .fs
        .get_stat_for_kind(&state.fs.inodes[file_inode].kind)
        .ok_or(__WASI_EIO));

    let buf_cell = wasi_try!(buf.deref(memory));
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd)).clone();
    let fd_inode = fd_entry.inode;
    if !has_rights(fd_entry.rights, __WASI_RIGHT_PATH_FILESTAT_SET_TIMES) {
        return __WASI_EACCES;
    }
    if (fst_flags & __WASI_FILESTAT_SET_ATIM != 0 && fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0)
        || (fst_flags & __WASI_FILESTAT_SET_MTIM != 0
            && fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0)
    {
        return __WASI_EINVAL;
    }

    let path_string = get_input_str!(memory, path, path_len);
    debug!("=> base_fd: {}, path: {}", fd, &path_string);

    let file_inode = wasi_try!(state.fs.get_inode_at_path(
        fd,
        path_string,
        flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    ));
    let stat = wasi_try!(state
        .fs
        .get_stat_for_kind(&state.fs.inodes[file_inode].kind)
        .ok_or(__WASI_EIO));

    let inode = &mut state.fs.inodes[fd_inode];

    if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 || fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.st_atim = time_to_set;
        // TODO: set it for more than just files
        match &mut inode.kind {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    handle.set_last_accessed(time_to_set);
                }
            }
            _ => {}
        }
    }
    if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 || fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.st_mtim = time_to_set;
        // TODO: set it for more than just files
        match &mut inode.kind {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    handle.set_last_modified(time_to_set);
                }
            }
            _ => {}
        }
    }

    __WASI_ESUCCESS
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
    if old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let old_path_str = get_input_str!(memory, old_path, old_path_len);
    let new_path_str = get_input_str!(memory, new_path, new_path_len);
    let source_fd = wasi_try!(state.fs.get_fd(old_fd));
    let target_fd = wasi_try!(state.fs.get_fd(new_fd));
    debug!(
        "=> source_fd: {}, source_path: {}, target_fd: {}, target_path: {}",
        old_fd, old_path_str, new_fd, new_path_str
    );

    if !(has_rights(source_fd.rights, __WASI_RIGHT_PATH_LINK_SOURCE)
        && has_rights(target_fd.rights, __WASI_RIGHT_PATH_LINK_TARGET))
    {
        return __WASI_EACCES;
    }

    let source_inode = wasi_try!(state.fs.get_inode_at_path(
        old_fd,
        old_path_str,
        old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    ));
    let target_path_arg = std::path::PathBuf::from(new_path_str);
    let (target_parent_inode, new_entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(new_fd, &target_path_arg, false));

    if state.fs.inodes[source_inode].stat.st_nlink == __wasi_linkcount_t::max_value() {
        return __WASI_EMLINK;
    }
    match &mut state.fs.inodes[target_parent_inode].kind {
        Kind::Dir { entries, .. } => {
            if entries.contains_key(&new_entry_name) {
                return __WASI_EEXIST;
            }
            entries.insert(new_entry_name, source_inode);
        }
        Kind::Root { .. } => return __WASI_EINVAL,
        Kind::File { .. } | Kind::Symlink { .. } | Kind::Buffer { .. } => return __WASI_ENOTDIR,
    }
    state.fs.inodes[source_inode].stat.st_nlink += 1;

    __WASI_ESUCCESS
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
    if dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    /* TODO: find actual upper bound on name size (also this is a path, not a name :think-fish:) */
    if path_len > 1024 * 1024 {
        return __WASI_ENAMETOOLONG;
    }

    let fd_cell = wasi_try!(fd.deref(memory));

    // o_flags:
    // - __WASI_O_CREAT (create if it does not exist)
    // - __WASI_O_DIRECTORY (fail if not dir)
    // - __WASI_O_EXCL (fail if file exists)
    // - __WASI_O_TRUNC (truncate size to 0)

    let working_dir = wasi_try!(state.fs.get_fd(dirfd));
    let working_dir_rights_inheriting = working_dir.rights_inheriting;

    // ASSUMPTION: open rights apply recursively
    if !has_rights(working_dir.rights, __WASI_RIGHT_PATH_OPEN) {
        return __WASI_EACCES;
    }
    let path_string = get_input_str!(memory, path, path_len);

    debug!("=> fd: {}, path: {}", dirfd, &path_string);

    let path_arg = std::path::PathBuf::from(path_string);
    let maybe_inode = state.fs.get_inode_at_path(
        dirfd,
        path_string,
        dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    );

    let mut open_flags = 0;
    // TODO: traverse rights of dirs properly
    // COMMENTED OUT: WASI isn't giving appropriate rights here when opening
    //              TODO: look into this; file a bug report if this is a bug
    let adjusted_rights = /*fs_rights_base &*/ working_dir_rights_inheriting;
    let inode = if let Ok(inode) = maybe_inode {
        // Happy path, we found the file we're trying to open
        match &mut state.fs.inodes[inode].kind {
            Kind::File {
                ref mut handle,
                path,
                fd,
            } => {
                if let Some(special_fd) = fd {
                    // short circuit if we're dealing with a special file
                    assert!(handle.is_some());
                    fd_cell.set(*special_fd);
                    return __WASI_ESUCCESS;
                }
                if o_flags & __WASI_O_DIRECTORY != 0 {
                    return __WASI_ENOTDIR;
                }
                if o_flags & __WASI_O_EXCL != 0 {
                    if path.exists() {
                        return __WASI_EEXIST;
                    }
                }
                let mut open_options = std::fs::OpenOptions::new();
                let write_permission = adjusted_rights & __WASI_RIGHT_FD_WRITE != 0;
                // append, truncate, and create all require the permission to write
                let (append_permission, truncate_permission, create_permission) =
                    if write_permission {
                        (
                            fs_flags & __WASI_FDFLAG_APPEND != 0,
                            o_flags & __WASI_O_TRUNC != 0,
                            o_flags & __WASI_O_CREAT != 0,
                        )
                    } else {
                        (false, false, false)
                    };
                let open_options = open_options
                    .read(true)
                    // TODO: ensure these rights are actually valid given parent, etc.
                    .write(write_permission)
                    .create(create_permission)
                    .append(append_permission)
                    .truncate(truncate_permission);
                open_flags |= Fd::READ;
                if adjusted_rights & __WASI_RIGHT_FD_WRITE != 0 {
                    open_flags |= Fd::WRITE;
                }
                if o_flags & __WASI_O_CREAT != 0 {
                    open_flags |= Fd::CREATE;
                }
                if o_flags & __WASI_O_TRUNC != 0 {
                    open_flags |= Fd::TRUNCATE;
                }
                *handle = Some(Box::new(HostFile::new(
                    wasi_try!(open_options.open(&path).map_err(|_| __WASI_EIO)),
                    path.to_path_buf(),
                    true,
                    adjusted_rights & __WASI_RIGHT_FD_WRITE != 0,
                    false,
                )));
            }
            Kind::Buffer { .. } => unimplemented!("wasi::path_open for Buffer type files"),
            Kind::Dir { .. } | Kind::Root { .. } => {
                // TODO: adjust these to be correct
                if o_flags & __WASI_O_EXCL != 0 {
                    if path_arg.exists() {
                        return __WASI_EEXIST;
                    }
                }
            }
            Kind::Symlink {
                base_po_dir,
                path_to_symlink,
                relative_path,
            } => {
                // I think this should return an error (because symlinks should be resolved away by the path traversal)
                // TODO: investigate this
                unimplemented!("SYMLINKS IN PATH_OPEN");
            }
        }
        inode
    } else {
        // less-happy path, we have to try to create the file
        debug!("Maybe creating file");
        if o_flags & __WASI_O_CREAT != 0 {
            if o_flags & __WASI_O_DIRECTORY != 0 {
                return __WASI_ENOTDIR;
            }
            debug!("Creating file");
            // strip end file name

            let (parent_inode, new_entity_name) = wasi_try!(state.fs.get_parent_inode_at_path(
                dirfd,
                &path_arg,
                dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0
            ));
            let new_file_host_path = match &state.fs.inodes[parent_inode].kind {
                Kind::Dir { path, .. } => {
                    let mut new_path = path.clone();
                    new_path.push(&new_entity_name);
                    new_path
                }
                Kind::Root { .. } => return __WASI_EACCES,
                _ => return __WASI_EINVAL,
            };
            // once we got the data we need from the parent, we lookup the host file
            // todo: extra check that opening with write access is okay
            let handle = {
                let mut open_options = std::fs::OpenOptions::new();
                let open_options = open_options
                    .read(true)
                    .append(fs_flags & __WASI_FDFLAG_APPEND != 0)
                    // TODO: ensure these rights are actually valid given parent, etc.
                    // write access is required for creating a file
                    .write(true)
                    .create_new(true);
                open_flags |= Fd::READ | Fd::WRITE | Fd::CREATE | Fd::TRUNCATE;

                Some(Box::new(HostFile::new(
                    wasi_try!(open_options.open(&new_file_host_path).map_err(|e| {
                        debug!("Error opening file {}", e);
                        __WASI_EIO
                    })),
                    new_file_host_path.clone(),
                    true,
                    true,
                    true,
                )) as Box<dyn WasiFile>)
            };

            let new_inode = {
                let kind = Kind::File {
                    handle,
                    path: new_file_host_path,
                    fd: None,
                };
                wasi_try!(state.fs.create_inode(kind, false, new_entity_name.clone()))
            };

            if let Kind::Dir {
                ref mut entries, ..
            } = &mut state.fs.inodes[parent_inode].kind
            {
                entries.insert(new_entity_name, new_inode);
            }

            new_inode
        } else {
            return maybe_inode.unwrap_err();
        }
    };

    debug!(
        "inode {:?} value {:#?} found!",
        inode, state.fs.inodes[inode]
    );

    // TODO: check and reduce these
    // TODO: ensure a mutable fd to root can never be opened
    let out_fd = wasi_try!(state.fs.create_fd(
        adjusted_rights,
        fs_rights_inheriting,
        fs_flags,
        open_flags,
        inode
    ));

    fd_cell.set(out_fd);
    debug!("wasi::path_open returning fd {}", out_fd);

    __WASI_ESUCCESS
}

/// ### `path_readlink()`
/// Read the value of a symlink
/// Inputs:
/// - `__wasi_fd_t dir_fd`
///     The base directory from which `path` is understood
/// - `const char *path`
///     Pointer to UTF-8 bytes that make up the path to the symlink
/// - `u32 path_len`
///     The number of bytes to read from `path`
/// - `u32 buf_len`
///     Space available pointed to by `buf`
/// Outputs:
/// - `char *buf`
///     Pointer to characters containing the path that the symlink points to
/// - `u32 buf_used`
///     The number of bytes written to `buf`
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let base_dir = wasi_try!(state.fs.fd_map.get(&dir_fd).ok_or(__WASI_EBADF));
    if !has_rights(base_dir.rights, __WASI_RIGHT_PATH_READLINK) {
        return __WASI_EACCES;
    }
    let path_str = get_input_str!(memory, path, path_len);
    let inode = wasi_try!(state.fs.get_inode_at_path(dir_fd, path_str, false));

    if let Kind::Symlink { relative_path, .. } = &state.fs.inodes[inode].kind {
        let rel_path_str = relative_path.to_string_lossy();
        debug!("Result => {:?}", rel_path_str);
        let bytes = rel_path_str.bytes();
        if bytes.len() >= buf_len as usize {
            return __WASI_EOVERFLOW;
        }

        let out = wasi_try!(buf.deref(memory, 0, buf_len));
        let mut bytes_written = 0;
        for b in bytes {
            out[bytes_written].set(b);
            bytes_written += 1;
        }
        // should we null terminate this?

        let bytes_out = wasi_try!(buf_used.deref(memory));
        bytes_out.set(bytes_written as u32);
    } else {
        return __WASI_EINVAL;
    }

    __WASI_ESUCCESS
}

/// Returns __WASI_ENOTEMTPY if directory is not empty
pub fn path_remove_directory(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    // TODO check if fd is a dir, ensure it's within sandbox, etc.
    debug!("wasi::path_remove_directory");
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let base_dir = wasi_try!(state.fs.fd_map.get(&fd), __WASI_EBADF);
    let path_str = get_input_str!(memory, path, path_len);

    let inode = wasi_try!(state.fs.get_inode_at_path(fd, path_str, false));
    let (parent_inode, childs_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(fd, std::path::Path::new(path_str), false));

    let host_path_to_remove = match &state.fs.inodes[inode].kind {
        Kind::Dir { entries, path, .. } => {
            if !entries.is_empty() {
                return __WASI_ENOTEMPTY;
            } else {
                if wasi_try!(std::fs::read_dir(path).ok(), __WASI_EIO).count() != 0 {
                    return __WASI_ENOTEMPTY;
                }
            }
            path.clone()
        }
        Kind::Root { .. } => return __WASI_EACCES,
        _ => return __WASI_ENOTDIR,
    };

    match &mut state.fs.inodes[parent_inode].kind {
        Kind::Dir {
            ref mut entries, ..
        } => {
            let removed_inode = wasi_try!(entries.remove(&childs_name).ok_or(__WASI_EINVAL));
            // TODO: make this a debug assert in the future
            assert!(inode == removed_inode);
        }
        Kind::Root { .. } => return __WASI_EACCES,
        _ => unreachable!(
            "Internal logic error in wasi::path_remove_directory, parent is not a directory"
        ),
    }

    if let Err(_) = std::fs::remove_dir(path_str) {
        // reinsert to prevent FS from being in bad state
        if let Kind::Dir {
            ref mut entries, ..
        } = &mut state.fs.inodes[parent_inode].kind
        {
            entries.insert(childs_name, inode);
        }
        // TODO: more intelligently return error value by inspecting returned error value
        return __WASI_EIO;
    }

    __WASI_ESUCCESS
}

/// ### `path_rename()`
/// Rename a file or directory
/// Inputs:
/// - `__wasi_fd_t old_fd`
///     The base directory for `old_path`
/// - `const char* old_path`
///     Pointer to UTF8 bytes, the file to be renamed
/// - `u32 old_path_len`
///     The number of bytes to read from `old_path`
/// - `__wasi_fd_t new_fd`
///     The base directory for `new_path`
/// - `const char* new_path`
///     Pointer to UTF8 bytes, the new file name
/// - `u32 new_path_len`
///     The number of bytes to read from `new_path`
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
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let source_str = get_input_str!(memory, old_path, old_path_len);
    let source_path = std::path::Path::new(source_str);
    let target_str = get_input_str!(memory, new_path, new_path_len);
    let target_path = std::path::Path::new(target_str);

    {
        let source_fd = wasi_try!(state.fs.get_fd(old_fd));
        if !has_rights(source_fd.rights, __WASI_RIGHT_PATH_RENAME_SOURCE) {
            return __WASI_EACCES;
        }
        let target_fd = wasi_try!(state.fs.get_fd(new_fd));
        if !has_rights(target_fd.rights, __WASI_RIGHT_PATH_RENAME_TARGET) {
            return __WASI_EACCES;
        }
    }

    let (source_parent_inode, source_entry_name) =
        wasi_try!(state.fs.get_parent_inode_at_path(old_fd, source_path, true));
    let (target_parent_inode, target_entry_name) =
        wasi_try!(state.fs.get_parent_inode_at_path(new_fd, target_path, true));

    let host_adjusted_target_path = match &state.fs.inodes[target_parent_inode].kind {
        Kind::Dir { entries, path, .. } => {
            if entries.contains_key(&target_entry_name) {
                return __WASI_EEXIST;
            }
            let mut out_path = path.clone();
            // remove fd's own name which will be double counted
            out_path.pop();
            out_path.push(target_path);
            out_path
        }
        Kind::Root { .. } => return __WASI_ENOTCAPABLE,
        Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
            unreachable!("Fatal internal logic error: parent of inode is not a directory")
        }
    };
    let source_entry = match &mut state.fs.inodes[source_parent_inode].kind {
        Kind::Dir { entries, .. } => wasi_try!(entries.remove(&source_entry_name), __WASI_EINVAL),
        Kind::Root { .. } => return __WASI_ENOTCAPABLE,
        Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
            unreachable!("Fatal internal logic error: parent of inode is not a directory")
        }
    };

    match &mut state.fs.inodes[source_entry].kind {
        Kind::File {
            handle,
            ref mut path,
            ..
        } => {
            let result = if let Some(h) = handle {
                h.rename_file(&host_adjusted_target_path)
                    .map_err(|e| e.into_wasi_err())
            } else {
                let out =
                    std::fs::rename(&path, &host_adjusted_target_path).map_err(|_| __WASI_EIO);
                *path = host_adjusted_target_path;
                out
            };
            // if the above operation failed we have to revert the previous change and then fail
            if let Err(e) = result {
                if let Kind::Dir { entries, .. } = &mut state.fs.inodes[source_parent_inode].kind {
                    entries.insert(source_entry_name, source_entry);
                    return e;
                }
            }
        }
        Kind::Dir { path, .. } => unimplemented!("wasi::path_rename on Directories"),
        Kind::Buffer { .. } => {}
        Kind::Symlink { .. } => {}
        Kind::Root { .. } => unreachable!("The root can not be moved"),
    }

    if let Kind::Dir { entries, .. } = &mut state.fs.inodes[target_parent_inode].kind {
        let result = entries.insert(target_entry_name, source_entry);
        assert!(
            result.is_none(),
            "Fatal error: race condition on filesystem detected or internal logic error"
        );
    }

    __WASI_ESUCCESS
}

/// ### `path_symlink()`
/// Create a symlink
/// Inputs:
/// - `const char *old_path`
///     Array of UTF-8 bytes representing the source path
/// - `u32 old_path_len`
///     The number of bytes to read from `old_path`
/// - `__wasi_fd_t fd`
///     The base directory from which the paths are understood
/// - `const char *new_path`
///     Array of UTF-8 bytes representing the target path
/// - `u32 new_path_len`
///     The number of bytes to read from `new_path`
pub fn path_symlink(
    ctx: &mut Ctx,
    old_path: WasmPtr<u8, Array>,
    old_path_len: u32,
    fd: __wasi_fd_t,
    new_path: WasmPtr<u8, Array>,
    new_path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_symlink");
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);
    let old_path_str = get_input_str!(memory, old_path, old_path_len);
    let new_path_str = get_input_str!(memory, new_path, new_path_len);
    let base_fd = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(base_fd.rights, __WASI_RIGHT_PATH_SYMLINK) {
        return __WASI_EACCES;
    }

    // get the depth of the parent + 1 (UNDER INVESTIGATION HMMMMMMMM THINK FISH ^ THINK FISH)
    let old_path_path = std::path::Path::new(old_path_str);
    let (source_inode, _) = wasi_try!(state.fs.get_parent_inode_at_path(fd, old_path_path, true));
    let depth = wasi_try!(state.fs.path_depth_from_fd(fd, source_inode)) - 1;

    let new_path_path = std::path::Path::new(new_path_str);
    let (target_parent_inode, entry_name) =
        wasi_try!(state.fs.get_parent_inode_at_path(fd, new_path_path, true));

    // short circuit if anything is wrong, before we create an inode
    match &state.fs.inodes[target_parent_inode].kind {
        Kind::Dir { entries, .. } => {
            if entries.contains_key(&entry_name) {
                return __WASI_EEXIST;
            }
        }
        Kind::Root { .. } => return __WASI_ENOTCAPABLE,
        Kind::File { .. } | Kind::Symlink { .. } | Kind::Buffer { .. } => {
            unreachable!("get_parent_inode_at_path returned something other than a Dir or Root")
        }
    }

    let mut source_path = std::path::Path::new(old_path_str);
    let mut relative_path = std::path::PathBuf::new();
    for _ in 0..depth {
        relative_path.push("..");
    }
    relative_path.push(source_path);
    debug!(
        "Symlinking {} to {}",
        new_path_str,
        relative_path.to_string_lossy()
    );

    let kind = Kind::Symlink {
        base_po_dir: fd,
        path_to_symlink: std::path::PathBuf::from(new_path_str),
        relative_path,
    };
    let new_inode = state
        .fs
        .create_inode_with_default_stat(kind, false, entry_name.clone());

    if let Kind::Dir {
        ref mut entries, ..
    } = &mut state.fs.inodes[target_parent_inode].kind
    {
        entries.insert(entry_name, new_inode);
    }

    __WASI_ESUCCESS
}

/// ### `path_unlink_file()`
/// Unlink a file, deleting if the number of hardlinks is 1
/// Inputs:
/// - `__wasi_fd_t fd`
///     The base file descriptor from which the path is understood
/// - `const char *path`
///     Array of UTF-8 bytes representing the path
/// - `u32 path_len`
///     The number of bytes in the `path` array
pub fn path_unlink_file(
    ctx: &mut Ctx,
    fd: __wasi_fd_t,
    path: WasmPtr<u8, Array>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_unlink_file");
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let base_dir = wasi_try!(state.fs.fd_map.get(&fd).ok_or(__WASI_EBADF));
    if !has_rights(base_dir.rights, __WASI_RIGHT_PATH_UNLINK_FILE) {
        return __WASI_EACCES;
    }
    let path_str = get_input_str!(memory, path, path_len);
    debug!("Requested file: {}", path_str);

    let inode = wasi_try!(state.fs.get_inode_at_path(fd, path_str, false));
    let (parent_inode, childs_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(fd, std::path::Path::new(path_str), false));

    let removed_inode = match &mut state.fs.inodes[parent_inode].kind {
        Kind::Dir {
            ref mut entries, ..
        } => {
            let removed_inode = wasi_try!(entries.remove(&childs_name).ok_or(__WASI_EINVAL));
            // TODO: make this a debug assert in the future
            assert!(inode == removed_inode);
            debug_assert!(state.fs.inodes[inode].stat.st_nlink > 0);
            removed_inode
        }
        Kind::Root { .. } => return __WASI_EACCES,
        _ => unreachable!(
            "Internal logic error in wasi::path_unlink_file, parent is not a directory"
        ),
    };

    state.fs.inodes[removed_inode].stat.st_nlink -= 1;
    if state.fs.inodes[removed_inode].stat.st_nlink == 0 {
        match &mut state.fs.inodes[removed_inode].kind {
            Kind::File { handle, path, .. } => {
                if let Some(h) = handle {
                    wasi_try!(h.unlink().map_err(WasiFsError::into_wasi_err));
                } else {
                    // File is closed
                    // problem with the abstraction, we can't call unlink because there's no handle
                    // TODO: replace this code
                    wasi_try!(std::fs::remove_file(path).map_err(|_| __WASI_EIO));
                }
            }
            Kind::Dir { .. } | Kind::Root { .. } => return __WASI_EISDIR,
            Kind::Symlink { .. } => {
                // TODO: actually delete real symlinks and do nothing for virtual symlinks
            }
            _ => unimplemented!("wasi::path_unlink_file for Buffer"),
        }
        // TODO: test this on Windows and actually make it portable
        // make the file an orphan fd if the fd is still open
        let fd_is_orphaned = if let Kind::File { handle, .. } = &state.fs.inodes[removed_inode].kind
        {
            handle.is_some()
        } else {
            false
        };
        let removed_inode_val = unsafe { state.fs.remove_inode(removed_inode) };
        assert!(
            removed_inode_val.is_some(),
            "Inode could not be removed because it doesn't exist"
        );

        if fd_is_orphaned {
            state
                .fs
                .orphan_fds
                .insert(removed_inode, removed_inode_val.unwrap());
        }
    }

    __WASI_ESUCCESS
}

/// ### `poll_oneoff()`
/// Concurrently poll for a set of events
/// Inputs:
/// - `const __wasi_subscription_t *in`
///     The events to subscribe to
/// - `__wasi_event_t *out`
///     The events that have occured
/// - `u32 nsubscriptions`
///     The number of subscriptions and the number of events
/// Output:
/// - `u32 nevents`
///     The number of events seen
pub fn poll_oneoff(
    ctx: &mut Ctx,
    in_: WasmPtr<__wasi_subscription_t, Array>,
    out_: WasmPtr<__wasi_event_t, Array>,
    nsubscriptions: u32,
    nevents: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::poll_oneoff");
    debug!("  => nsubscriptions = {}", nsubscriptions);
    let (memory, state) = get_memory_and_wasi_state(ctx, 0);

    let subscription_array = wasi_try!(in_.deref(memory, 0, nsubscriptions));
    let event_array = wasi_try!(out_.deref(memory, 0, nsubscriptions));
    let mut events_seen = 0;
    let out_ptr = wasi_try!(nevents.deref(memory));

    let mut fds = vec![];
    let mut clock_subs = vec![];
    let mut in_events = vec![];
    let mut total_ns_slept = 0;

    for sub in subscription_array.iter() {
        let s: WasiSubscription = wasi_try!(sub.get().try_into());
        let mut peb = PollEventBuilder::new();
        let mut ns_to_sleep = 0;

        let fd = match s.event_type {
            EventType::Read(__wasi_subscription_fs_readwrite_t { fd }) => {
                match fd {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    _ => {
                        let fd_entry = wasi_try!(state.fs.get_fd(fd));
                        if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_READ) {
                            return __WASI_EACCES;
                        }
                    }
                }
                in_events.push(peb.add(PollEvent::PollIn).build());
                Some(fd)
            }
            EventType::Write(__wasi_subscription_fs_readwrite_t { fd }) => {
                match fd {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    _ => {
                        let fd_entry = wasi_try!(state.fs.get_fd(fd));

                        if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE) {
                            return __WASI_EACCES;
                        }
                    }
                }
                in_events.push(peb.add(PollEvent::PollOut).build());
                Some(fd)
            }
            EventType::Clock(clock_info) => {
                if clock_info.clock_id == __WASI_CLOCK_REALTIME {
                    // this is a hack
                    // TODO: do this properly
                    ns_to_sleep = clock_info.timeout;
                    clock_subs.push(clock_info);
                    None
                } else {
                    unimplemented!("Polling not implemented for clocks yet");
                }
            }
        };

        if let Some(fd) = fd {
            let wasi_file_ref: &dyn WasiFile = match fd {
                __WASI_STDERR_FILENO => wasi_try!(
                    wasi_try!(state.fs.stderr().map_err(WasiFsError::into_wasi_err)).as_ref(),
                    __WASI_EBADF
                )
                .as_ref(),
                __WASI_STDIN_FILENO => wasi_try!(
                    wasi_try!(state.fs.stdin().map_err(WasiFsError::into_wasi_err)).as_ref(),
                    __WASI_EBADF
                )
                .as_ref(),
                __WASI_STDOUT_FILENO => wasi_try!(
                    wasi_try!(state.fs.stdout().map_err(WasiFsError::into_wasi_err)).as_ref(),
                    __WASI_EBADF
                )
                .as_ref(),
                _ => {
                    let fd_entry = wasi_try!(state.fs.get_fd(fd));
                    let inode = fd_entry.inode;
                    if !has_rights(fd_entry.rights, __WASI_RIGHT_POLL_FD_READWRITE) {
                        return __WASI_EACCES;
                    }

                    match &state.fs.inodes[inode].kind {
                        Kind::File { handle, .. } => {
                            if let Some(h) = handle {
                                h.as_ref()
                            } else {
                                return __WASI_EBADF;
                            }
                        }
                        Kind::Dir { .. }
                        | Kind::Root { .. }
                        | Kind::Buffer { .. }
                        | Kind::Symlink { .. } => {
                            unimplemented!("polling read on non-files not yet supported")
                        }
                    }
                }
            };
            fds.push(wasi_file_ref);
        } else {
            let remaining_ns = ns_to_sleep as i64 - total_ns_slept as i64;
            if remaining_ns > 0 {
                debug!("Sleeping for {} nanoseconds", remaining_ns);
                let duration = std::time::Duration::from_nanos(remaining_ns as u64);
                std::thread::sleep(duration);
                total_ns_slept += remaining_ns;
            }
        }
    }
    let mut seen_events = vec![Default::default(); in_events.len()];
    wasi_try!(poll(
        fds.as_slice(),
        in_events.as_slice(),
        seen_events.as_mut_slice()
    )
    .map_err(|e| e.into_wasi_err()));

    for (i, seen_event) in seen_events.into_iter().enumerate() {
        let mut flags = 0;
        let mut error = __WASI_EAGAIN;
        let mut bytes_available = 0;
        let event_iter = iterate_poll_events(seen_event);
        for event in event_iter {
            match event {
                PollEvent::PollError => error = __WASI_EIO,
                PollEvent::PollHangUp => flags = __WASI_EVENT_FD_READWRITE_HANGUP,
                PollEvent::PollInvalid => error = __WASI_EINVAL,
                PollEvent::PollIn => {
                    bytes_available =
                        wasi_try!(fds[i].bytes_available().map_err(|e| e.into_wasi_err()));
                    error = __WASI_ESUCCESS;
                }
                PollEvent::PollOut => {
                    bytes_available =
                        wasi_try!(fds[i].bytes_available().map_err(|e| e.into_wasi_err()));
                    error = __WASI_ESUCCESS;
                }
            }
        }
        let event = __wasi_event_t {
            userdata: subscription_array[i].get().userdata,
            error,
            type_: subscription_array[i].get().type_,
            u: unsafe {
                __wasi_event_u {
                    fd_readwrite: __wasi_event_fd_readwrite_t {
                        nbytes: bytes_available as u64,
                        flags,
                    },
                }
            },
        };
        event_array[events_seen].set(event);
        events_seen += 1;
    }
    for clock_info in clock_subs {
        let event = __wasi_event_t {
            // TOOD: review userdata value
            userdata: 0,
            error: __WASI_ESUCCESS,
            type_: __WASI_EVENTTYPE_CLOCK,
            u: unsafe {
                __wasi_event_u {
                    fd_readwrite: __wasi_event_fd_readwrite_t {
                        nbytes: 0,
                        flags: 0,
                    },
                }
            },
        };
        event_array[events_seen].set(event);
        events_seen += 1;
    }
    out_ptr.set(events_seen as u32);
    __WASI_ESUCCESS
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
    let memory = ctx.memory(0);

    let buf = wasi_try!(buf.deref(memory, 0, buf_len));

    let res = unsafe {
        let u8_buffer = &mut *(buf as *const [_] as *mut [_] as *mut [u8]);
        getrandom::getrandom(u8_buffer)
    };
    match res {
        Ok(()) => __WASI_ESUCCESS,
        Err(_) => __WASI_EIO,
    }
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
