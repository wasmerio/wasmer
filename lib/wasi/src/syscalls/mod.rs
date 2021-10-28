#![allow(unused, clippy::too_many_arguments, clippy::cognitive_complexity)]

pub mod types {
    pub use wasmer_wasi_types::*;
}

#[cfg(any(
    target_os = "freebsd",
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple"
))]
pub mod unix;
#[cfg(any(target_arch = "wasm32"))]
pub mod wasm32;
#[cfg(any(target_os = "windows"))]
pub mod windows;

pub mod legacy;

use self::types::*;
use crate::utils::map_io_err;
use crate::{
    mem_error_to_wasi,
    state::{
        self, fs_error_into_wasi_err, iterate_poll_events, poll,
        virtual_file_type_to_wasi_file_type, Fd, Inode, InodeVal, Kind, PollEvent,
        PollEventBuilder, WasiState, MAX_SYMLINKS,
    },
    WasiEnv, WasiError, WasiThread,
};
use std::borrow::Borrow;
use std::convert::{Infallible, TryInto};
use std::io::{self, Read, Seek, Write};
use std::time::Duration;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::Ordering;
use tracing::{debug, trace};
use wasmer::{Memory, RuntimeError, Value, WasmPtr, WasmSlice};
use wasmer_vfs::{FsError, VirtualFile};

#[cfg(any(
    target_os = "freebsd",
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple"
))]
pub use unix::*;

#[cfg(any(target_os = "windows"))]
pub use windows::*;

#[cfg(any(target_arch = "wasm32"))]
pub use wasm32::*;

fn write_bytes_inner<T: Write>(
    mut write_loc: T,
    memory: &Memory,
    iovs_arr_cell: WasmSlice<__wasi_ciovec_t>,
) -> Result<u32, __wasi_errno_t> {
    let mut bytes_written = 0;
    for iov in iovs_arr_cell.iter() {
        let iov_inner = iov.read().map_err(mem_error_to_wasi)?;
        let bytes = WasmPtr::<u8>::new(iov_inner.buf)
            .slice(memory, iov_inner.buf_len)
            .map_err(mem_error_to_wasi)?;
        let bytes = bytes.read_to_vec().map_err(mem_error_to_wasi)?;
        write_loc.write_all(&bytes).map_err(map_io_err)?;

        bytes_written += iov_inner.buf_len;
    }
    Ok(bytes_written)
}

fn write_bytes<T: Write>(
    mut write_loc: T,
    memory: &Memory,
    iovs_arr: WasmSlice<__wasi_ciovec_t>,
) -> Result<u32, __wasi_errno_t> {
    let result = write_bytes_inner(&mut write_loc, memory, iovs_arr);
    write_loc.flush();
    result
}

fn read_bytes<T: Read>(
    mut reader: T,
    memory: &Memory,
    iovs_arr: WasmSlice<__wasi_iovec_t>,
) -> Result<u32, __wasi_errno_t> {
    let mut bytes_read = 0;

    // We allocate the raw_bytes first once instead of
    // N times in the loop.
    let mut raw_bytes: Vec<u8> = vec![0; 1024];

    for iov in iovs_arr.iter() {
        let iov_inner = iov.read().map_err(mem_error_to_wasi)?;
        raw_bytes.clear();
        raw_bytes.resize(iov_inner.buf_len as usize, 0);
        bytes_read += reader.read(&mut raw_bytes).map_err(map_io_err)? as u32;

        let buf = WasmPtr::<u8>::new(iov_inner.buf)
            .slice(memory, iov_inner.buf_len)
            .map_err(mem_error_to_wasi)?;
        buf.write_slice(&raw_bytes).map_err(mem_error_to_wasi)?;
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
    ptr_buffer: WasmPtr<WasmPtr<u8>>,
    buffer: WasmPtr<u8>,
) -> __wasi_errno_t {
    let ptrs = wasi_try_mem!(ptr_buffer.slice(memory, from.len() as u32));

    let mut current_buffer_offset = 0;
    for ((i, sub_buffer), ptr) in from.iter().enumerate().zip(ptrs.iter()) {
        trace!("ptr: {:?}, subbuffer: {:?}", ptr, sub_buffer);
        let new_ptr = WasmPtr::new(buffer.offset() + current_buffer_offset);
        wasi_try_mem!(ptr.write(new_ptr));

        let data = wasi_try_mem!(new_ptr.slice(memory, sub_buffer.len() as u32));
        wasi_try_mem!(data.write_slice(sub_buffer));
        wasi_try_mem!(wasi_try_mem!(new_ptr.add(sub_buffer.len() as u32)).write(memory, 0));

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
    thread: &WasiThread,
    argv: WasmPtr<WasmPtr<u8>>,
    argv_buf: WasmPtr<u8>,
) -> __wasi_errno_t {
    debug!("wasi::args_get");
    let (memory, state) = thread.get_memory_and_wasi_state(0);

    let result = write_buffer_array(memory, &*state.args, argv, argv_buf);

    debug!(
        "=> args:\n{}",
        state
            .args
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{:>20}: {}", i, ::std::str::from_utf8(v).unwrap()))
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
    thread: &WasiThread,
    argc: WasmPtr<u32>,
    argv_buf_size: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::args_sizes_get");
    let (memory, state) = thread.get_memory_and_wasi_state(0);

    let argc = argc.deref(memory);
    let argv_buf_size = argv_buf_size.deref(memory);

    let argc_val = state.args.len() as u32;
    let argv_buf_size_val = state.args.iter().map(|v| v.len() as u32 + 1).sum();
    wasi_try_mem!(argc.write(argc_val));
    wasi_try_mem!(argv_buf_size.write(argv_buf_size_val));

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
    thread: &WasiThread,
    clock_id: __wasi_clockid_t,
    resolution: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    trace!("wasi::clock_res_get");
    let memory = thread.memory();

    let out_addr = resolution.deref(memory);
    let t_out = wasi_try!(platform_clock_res_get(clock_id, out_addr));
    wasi_try_mem!(resolution.write(memory, t_out as __wasi_timestamp_t));
    __WASI_ESUCCESS
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
    thread: &WasiThread,
    clock_id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t>,
) -> __wasi_errno_t {
    debug!(
        "wasi::clock_time_get clock_id: {}, precision: {}",
        clock_id,
        precision
    );
    let memory = thread.memory();

    let out_addr = time.deref(memory);
    let t_out = wasi_try!(platform_clock_time_get(clock_id, precision));
    wasi_try_mem!(time.write(memory, t_out as __wasi_timestamp_t));

    let result = __WASI_ESUCCESS;
    trace!(
        "time: {} => {}",
        wasi_try_mem!(time.deref(memory).read()),
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
    thread: &WasiThread,
    environ: WasmPtr<WasmPtr<u8>>,
    environ_buf: WasmPtr<u8>,
) -> __wasi_errno_t {
    debug!(
        "wasi::environ_get. Environ: {:?}, environ_buf: {:?}",
        environ, environ_buf
    );
    let (memory, state) = thread.get_memory_and_wasi_state(0);
    trace!(" -> State envs: {:?}", state.envs);

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
    thread: &WasiThread,
    environ_count: WasmPtr<u32>,
    environ_buf_size: WasmPtr<u32>,
) -> __wasi_errno_t {
    trace!("wasi::environ_sizes_get");
    let (memory, state) = thread.get_memory_and_wasi_state(0);

    let environ_count = environ_count.deref(memory);
    let environ_buf_size = environ_buf_size.deref(memory);

    let env_var_count = state.envs.len() as u32;
    let env_buf_size = state.envs.iter().map(|v| v.len() as u32 + 1).sum();
    wasi_try_mem!(environ_count.write(env_var_count));
    wasi_try_mem!(environ_buf_size.write(env_buf_size));

    trace!(
        "env_var_count: {}, env_buf_size: {}",
        env_var_count,
        env_buf_size
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
    thread: &WasiThread,
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    offset: __wasi_filesize_t,
    len: __wasi_filesize_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_allocate");
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_ALLOCATE) {
        return __WASI_EACCES;
    }
    let new_size = wasi_try!(offset.checked_add(len).ok_or(__WASI_EINVAL));
    {
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    wasi_try!(handle.set_len(new_size).map_err(fs_error_into_wasi_err));
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
    }
    inodes.arena[inode].stat.write().unwrap().st_size = new_size;
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
pub fn fd_close(thread: &WasiThread, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_close: fd={}", fd);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);

    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    wasi_try!(state.fs.close_fd(inodes.deref(), fd));

    __WASI_ESUCCESS
}

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to sync
pub fn fd_datasync(thread: &WasiThread, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_datasync");
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_DATASYNC) {
        return __WASI_EACCES;
    }

    if let Err(e) = state.fs.flush(inodes.deref(), fd) {
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf_ptr: WasmPtr<__wasi_fdstat_t>,
) -> __wasi_errno_t {
    debug!(
        "wasi::fd_fdstat_get: fd={}, buf_ptr={}",
        fd,
        buf_ptr.offset()
    );
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let stat = wasi_try!(state.fs.fdstat(inodes.deref(), fd));
    let buf = buf_ptr.deref(memory);

    wasi_try_mem!(buf.write(stat));

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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    flags: __wasi_fdflags_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_fdstat_set_flags");
    let (memory, state) = thread.get_memory_and_wasi_state(0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    fs_rights_base: __wasi_rights_t,
    fs_rights_inheriting: __wasi_rights_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_fdstat_set_rights");
    let (memory, state) = thread.get_memory_and_wasi_state(0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(__WASI_EBADF));

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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_filestat_t>,
) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_get");
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_FILESTAT_GET) {
        return __WASI_EACCES;
    }

    let stat = wasi_try!(state.fs.filestat_fd(inodes.deref(), fd));

    let buf = buf.deref(memory);
    wasi_try_mem!(buf.write(stat));

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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    st_size: __wasi_filesize_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_set_size");
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_FILESTAT_SET_SIZE) {
        return __WASI_EACCES;
    }

    {
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    wasi_try!(handle.set_len(st_size).map_err(fs_error_into_wasi_err));
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
    }
    inodes.arena[inode].stat.write().unwrap().st_size = st_size;

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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_set_times");
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_FILESTAT_SET_TIMES) {
        return __WASI_EACCES;
    }

    if (fst_flags & __WASI_FILESTAT_SET_ATIM != 0 && fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0)
        || (fst_flags & __WASI_FILESTAT_SET_MTIM != 0
            && fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0)
    {
        return __WASI_EINVAL;
    }

    let inode_idx = fd_entry.inode;
    let inode = &inodes.arena[inode_idx];

    if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 || fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_atim = time_to_set;
    }

    if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 || fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_mtim = time_to_set;
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t>,
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nread: WasmPtr<u32>,
) -> Result<__wasi_errno_t, WasiError> {
    trace!("wasi::fd_pread: fd={}, offset={}", fd, offset);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);

    let iovs = wasi_try_mem_ok!(iovs.slice(memory, iovs_len));
    let nread_ref = nread.deref(memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_read = match fd {
        __WASI_STDIN_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdin_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                thread
            );
            if let Some(ref mut stdin) = guard.deref_mut() {
                wasi_try_ok!(read_bytes(stdin, memory, iovs), thread)
            } else {
                return Ok(__WASI_EBADF);
            }
        }
        __WASI_STDOUT_FILENO => return Ok(__WASI_EINVAL),
        __WASI_STDERR_FILENO => return Ok(__WASI_EINVAL),
        _ => {
            let inode = fd_entry.inode;

            if !(has_rights(fd_entry.rights, __WASI_RIGHT_FD_READ)
                && has_rights(fd_entry.rights, __WASI_RIGHT_FD_SEEK))
            {
                debug!(
                    "Invalid rights on {:X}: expected READ and SEEK",
                    fd_entry.rights
                );
                return Ok(__WASI_EACCES);
            }
            let mut guard = inodes.arena[inode].write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(h) = handle {
                        wasi_try_ok!(
                            h.seek(std::io::SeekFrom::Start(offset as u64))
                                .map_err(map_io_err),
                            thread
                        );
                        wasi_try_ok!(read_bytes(h, memory, iovs), thread)
                    } else {
                        return Ok(__WASI_EINVAL);
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => return Ok(__WASI_EISDIR),
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_pread"),
                Kind::Buffer { buffer } => {
                    wasi_try_ok!(
                        read_bytes(&buffer[(offset as usize)..], memory, iovs),
                        thread
                    )
                }
            }
        }
    };

    wasi_try_mem_ok!(nread_ref.write(bytes_read));
    debug!("Success: {} bytes read", bytes_read);
    Ok(__WASI_ESUCCESS)
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf: WasmPtr<__wasi_prestat_t>,
) -> __wasi_errno_t {
    trace!("wasi::fd_prestat_get: fd={}", fd);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);

    let prestat_ptr = buf.deref(memory);

    wasi_try_mem!(prestat_ptr.write(wasi_try!(state.fs.prestat_fd(inodes.deref(), fd))));

    __WASI_ESUCCESS
}

pub fn fd_prestat_dir_name(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8>,
    path_len: u32,
) -> __wasi_errno_t {
    trace!(
        "wasi::fd_prestat_dir_name: fd={}, path_len={}",
        fd,
        path_len
    );
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let path_chars = wasi_try_mem!(path.slice(memory, path_len));

    let real_inode = wasi_try!(state.fs.get_fd_inode(fd));
    let inode_val = &inodes.arena[real_inode];

    // check inode-val.is_preopened?

    trace!("=> inode: {:?}", inode_val);
    let guard = inode_val.read();
    match guard.deref() {
        Kind::Dir { .. } | Kind::Root { .. } => {
            // TODO: verify this: null termination, etc
            if inode_val.name.len() < path_len as usize {
                wasi_try_mem!(path_chars
                    .subslice(0..inode_val.name.len() as u64)
                    .write_slice(inode_val.name.as_bytes()));
                wasi_try_mem!(path_chars.index(inode_val.name.len() as u64).write(0));

                trace!("=> result: \"{}\"", inode_val.name);

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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t>,
    iovs_len: u32,
    offset: __wasi_filesize_t,
    nwritten: WasmPtr<u32>,
) -> Result<__wasi_errno_t, WasiError> {
    trace!("wasi::fd_pwrite");
    // TODO: refactor, this is just copied from `fd_write`...
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let iovs_arr = wasi_try_mem_ok!(iovs.slice(memory, iovs_len));
    let nwritten_ref = nwritten.deref(memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_written = match fd {
        __WASI_STDIN_FILENO => return Ok(__WASI_EINVAL),
        __WASI_STDOUT_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdout_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                thread
            );
            if let Some(ref mut stdout) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stdout, memory, iovs_arr), thread)
            } else {
                return Ok(__WASI_EBADF);
            }
        }
        __WASI_STDERR_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stderr_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                thread
            );
            if let Some(ref mut stderr) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stderr, memory, iovs_arr), thread)
            } else {
                return Ok(__WASI_EBADF);
            }
        }
        _ => {
            if !(has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE)
                && has_rights(fd_entry.rights, __WASI_RIGHT_FD_SEEK))
            {
                return Ok(__WASI_EACCES);
            }

            let inode_idx = fd_entry.inode;
            let inode = &inodes.arena[inode_idx];

            let mut guard = inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        wasi_try_ok!(
                            handle
                                .seek(std::io::SeekFrom::Start(offset as u64))
                                .map_err(map_io_err),
                            thread
                        );
                        wasi_try_ok!(write_bytes(handle, memory, iovs_arr), thread)
                    } else {
                        return Ok(__WASI_EINVAL);
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(__WASI_EISDIR);
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_pwrite"),
                Kind::Buffer { buffer } => {
                    wasi_try_ok!(
                        write_bytes(&mut buffer[(offset as usize)..], memory, iovs_arr),
                        thread
                    )
                }
            }
        }
    };

    wasi_try_mem_ok!(nwritten_ref.write(bytes_written));

    Ok(__WASI_ESUCCESS)
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_iovec_t>,
    iovs_len: u32,
    nread: WasmPtr<u32>,
) -> Result<__wasi_errno_t, WasiError> {
    trace!("wasi::fd_read: fd={}", fd);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);

    let iovs_arr = wasi_try_mem_ok!(iovs.slice(memory, iovs_len));
    let nread_ref = nread.deref(memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_read = match fd {
        __WASI_STDIN_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdin_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                thread
            );
            if let Some(ref mut stdin) = guard.deref_mut() {
                wasi_try_ok!(read_bytes(stdin, memory, iovs_arr), thread)
            } else {
                return Ok(__WASI_EBADF);
            }
        }
        __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => return Ok(__WASI_EINVAL),
        _ => {
            if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_READ) {
                // TODO: figure out the error to return when lacking rights
                return Ok(__WASI_EACCES);
            }

            let offset = fd_entry.offset as usize;
            let inode_idx = fd_entry.inode;
            let inode = &inodes.arena[inode_idx];

            let bytes_read = {
                let mut guard = inode.write();
                match guard.deref_mut() {
                    Kind::File { handle, .. } => {
                        if let Some(handle) = handle {
                            wasi_try_ok!(
                                handle
                                    .seek(std::io::SeekFrom::Start(offset as u64))
                                    .map_err(map_io_err),
                                thread
                            );
                            wasi_try_ok!(read_bytes(handle, memory, iovs_arr), thread)
                        } else {
                            return Ok(__WASI_EINVAL);
                        }
                    }
                    Kind::Dir { .. } | Kind::Root { .. } => {
                        // TODO: verify
                        return Ok(__WASI_EISDIR);
                    }
                    Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_read"),
                    Kind::Buffer { buffer } => {
                        wasi_try_ok!(read_bytes(&buffer[offset..], memory, iovs_arr), thread)
                    }
                }
            };

            // reborrow
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(__WASI_EBADF));
            fd_entry.offset += bytes_read as u64;

            bytes_read
        }
    };

    wasi_try_mem_ok!(nread_ref.write(bytes_read));

    Ok(__WASI_ESUCCESS)
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    buf: WasmPtr<u8>,
    buf_len: u32,
    cookie: __wasi_dircookie_t,
    bufused: WasmPtr<u32>,
) -> __wasi_errno_t {
    trace!("wasi::fd_readdir");
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    // TODO: figure out how this is supposed to work;
    // is it supposed to pack the buffer full every time until it can't? or do one at a time?

    let buf_arr = wasi_try_mem!(buf.slice(memory, buf_len));
    let bufused_ref = bufused.deref(memory);
    let working_dir = wasi_try!(state.fs.get_fd(fd));
    let mut cur_cookie = cookie;
    let mut buf_idx = 0;

    let entries: Vec<(String, u8, u64)> = {
        let guard = inodes.arena[working_dir.inode].read();
        match guard.deref() {
            Kind::Dir { path, entries, .. } => {
                debug!("Reading dir {:?}", path);
                // TODO: refactor this code
                // we need to support multiple calls,
                // simple and obviously correct implementation for now:
                // maintain consistent order via lexacographic sorting
                let fs_info = wasi_try!(wasi_try!(state.fs_read_dir(path))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| fs_error_into_wasi_err(e)));
                let mut entry_vec = wasi_try!(fs_info
                    .into_iter()
                    .map(|entry| {
                        let filename = entry.file_name().to_string_lossy().to_string();
                        debug!("Getting file: {:?}", filename);
                        let filetype = virtual_file_type_to_wasi_file_type(
                            entry.file_type().map_err(fs_error_into_wasi_err)?,
                        );
                        Ok((
                            filename, filetype, 0, // TODO: inode
                        ))
                    })
                    .collect::<Result<Vec<(String, u8, u64)>, _>>());
                entry_vec.extend(
                    entries
                        .iter()
                        .filter(|(_, inode)| inodes.arena[**inode].is_preopened)
                        .map(|(name, inode)| {
                            let entry = &inodes.arena[*inode];
                            let stat = entry.stat.read().unwrap();
                            (entry.name.to_string(), stat.st_filetype, stat.st_ino)
                        }),
                );
                entry_vec.sort_by(|a, b| a.0.cmp(&b.0));
                entry_vec
            }
            Kind::Root { entries } => {
                debug!("Reading root");
                let sorted_entries = {
                    let mut entry_vec: Vec<(String, Inode)> =
                        entries.iter().map(|(a, b)| (a.clone(), *b)).collect();
                    entry_vec.sort_by(|a, b| a.0.cmp(&b.0));
                    entry_vec
                };
                sorted_entries
                    .into_iter()
                    .map(|(name, inode)| {
                        let entry = &inodes.arena[inode];
                        let stat = entry.stat.read().unwrap();
                        (format!("/{}", entry.name), stat.st_filetype, stat.st_ino)
                    })
                    .collect()
            }
            Kind::File { .. } | Kind::Symlink { .. } | Kind::Buffer { .. } => {
                return __WASI_ENOTDIR
            }
        }
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
            wasi_try_mem!(buf_arr.index((i + buf_idx) as u64).write(dirent_bytes[i]));
        }
        buf_idx += upper_limit;
        if upper_limit != std::mem::size_of::<__wasi_dirent_t>() {
            break;
        }
        let upper_limit = std::cmp::min(buf_len as usize - buf_idx, namlen);
        for (i, b) in entry_path_str.bytes().take(upper_limit).enumerate() {
            wasi_try_mem!(buf_arr.index((i + buf_idx) as u64).write(b));
        }
        buf_idx += upper_limit;
        if upper_limit != namlen {
            break;
        }
    }

    wasi_try_mem!(bufused_ref.write(buf_idx as u32));
    __WASI_ESUCCESS
}

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `__wasi_fd_t from`
///     File descriptor to copy
/// - `__wasi_fd_t to`
///     Location to copy file descriptor to
pub fn fd_renumber(thread: &WasiThread, from: __wasi_fd_t, to: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_renumber: from={}, to={}", from, to);
    let (memory, state) = thread.get_memory_and_wasi_state(0);

    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&from).ok_or(__WASI_EBADF));

    let new_fd_entry = Fd {
        // TODO: verify this is correct
        rights: fd_entry.rights_inheriting,
        ..*fd_entry
    };

    fd_map.insert(to, new_fd_entry);
    fd_map.remove(&from);
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    offset: __wasi_filedelta_t,
    whence: __wasi_whence_t,
    newoffset: WasmPtr<__wasi_filesize_t>,
) -> Result<__wasi_errno_t, WasiError> {
    trace!("wasi::fd_seek: fd={}, offset={}", fd, offset);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let new_offset_ref = newoffset.deref(memory);
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_SEEK) {
        return Ok(__WASI_EACCES);
    }

    // TODO: handle case if fd is a dir?
    match whence {
        __WASI_WHENCE_CUR => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(__WASI_EBADF));
            fd_entry.offset = (fd_entry.offset as i64 + offset) as u64
        }
        __WASI_WHENCE_END => {
            use std::io::SeekFrom;
            let inode_idx = fd_entry.inode;
            let mut guard = inodes.arena[inode_idx].write();
            match guard.deref_mut() {
                Kind::File { ref mut handle, .. } => {
                    if let Some(handle) = handle {
                        let end =
                            wasi_try_ok!(handle.seek(SeekFrom::End(0)).map_err(map_io_err), thread);

                        // TODO: handle case if fd_entry.offset uses 64 bits of a u64
                        drop(guard);
                        let mut fd_map = state.fs.fd_map.write().unwrap();
                        let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(__WASI_EBADF));
                        fd_entry.offset = (end as i64 + offset) as u64;
                    } else {
                        return Ok(__WASI_EINVAL);
                    }
                }
                Kind::Symlink { .. } => {
                    unimplemented!("wasi::fd_seek not implemented for symlinks")
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: check this
                    return Ok(__WASI_EINVAL);
                }
                Kind::Buffer { .. } => {
                    // seeking buffers probably makes sense
                    // TODO: implement this
                    return Ok(__WASI_EINVAL);
                }
            }
        }
        __WASI_WHENCE_SET => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(__WASI_EBADF));
            fd_entry.offset = offset as u64
        }
        _ => return Ok(__WASI_EINVAL),
    }
    // reborrow
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    wasi_try_mem_ok!(new_offset_ref.write(fd_entry.offset));

    Ok(__WASI_ESUCCESS)
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
pub fn fd_sync(thread: &WasiThread, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_sync");
    debug!("=> fd={}", fd);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_SYNC) {
        return __WASI_EACCES;
    }
    let inode = fd_entry.inode;

    // TODO: implement this for more than files
    {
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(h) = handle {
                    wasi_try!(h.sync_to_disk().map_err(fs_error_into_wasi_err));
                } else {
                    return __WASI_EINVAL;
                }
            }
            Kind::Root { .. } | Kind::Dir { .. } => return __WASI_EISDIR,
            Kind::Buffer { .. } | Kind::Symlink { .. } => return __WASI_EINVAL,
        }
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    offset: WasmPtr<__wasi_filesize_t>,
) -> __wasi_errno_t {
    debug!("wasi::fd_tell");
    let (memory, state) = thread.get_memory_and_wasi_state(0);
    let offset_ref = offset.deref(memory);

    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_TELL) {
        return __WASI_EACCES;
    }

    wasi_try_mem!(offset_ref.write(fd_entry.offset));

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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    iovs: WasmPtr<__wasi_ciovec_t>,
    iovs_len: u32,
    nwritten: WasmPtr<u32>,
) -> Result<__wasi_errno_t, WasiError> {
    trace!("wasi::fd_write: fd={}", fd);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);
    let iovs_arr = wasi_try_mem_ok!(iovs.slice(memory, iovs_len));
    let nwritten_ref = nwritten.deref(memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_written = match fd {
        __WASI_STDIN_FILENO => return Ok(__WASI_EINVAL),
        __WASI_STDOUT_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdout_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                thread
            );
            if let Some(ref mut stdout) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stdout, memory, iovs_arr), thread)
            } else {
                return Ok(__WASI_EBADF);
            }
        }
        __WASI_STDERR_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stderr_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                thread
            );
            if let Some(ref mut stderr) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stderr, memory, iovs_arr), thread)
            } else {
                return Ok(__WASI_EBADF);
            }
        }
        _ => {
            if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE) {
                return Ok(__WASI_EACCES);
            }

            let offset = fd_entry.offset as usize;
            let inode_idx = fd_entry.inode;
            let inode = &inodes.arena[inode_idx];

            let bytes_written = {
                let mut guard = inode.write();
                match guard.deref_mut() {
                    Kind::File { handle, .. } => {
                        if let Some(handle) = handle {
                            wasi_try_ok!(
                                handle
                                    .seek(std::io::SeekFrom::Start(offset as u64))
                                    .map_err(map_io_err),
                                thread
                            );
                            wasi_try_ok!(write_bytes(handle, memory, iovs_arr), thread)
                        } else {
                            return Ok(__WASI_EINVAL);
                        }
                    }
                    Kind::Dir { .. } | Kind::Root { .. } => {
                        // TODO: verify
                        return Ok(__WASI_EISDIR);
                    }
                    Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_write"),
                    Kind::Buffer { buffer } => {
                        wasi_try_ok!(write_bytes(&mut buffer[offset..], memory, iovs_arr), thread)
                    }
                }
            };

            // reborrow
            {
                let mut fd_map = state.fs.fd_map.write().unwrap();
                let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(__WASI_EBADF));
                fd_entry.offset += bytes_written as u64;
            }
            wasi_try_ok!(state.fs.filestat_resync_size(inodes.deref(), fd), thread);

            bytes_written
        }
    };

    wasi_try_mem_ok!(nwritten_ref.write(bytes_written));

    Ok(__WASI_ESUCCESS)
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_create_directory");
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);

    let working_dir = wasi_try!(state.fs.get_fd(fd));
    {
        let guard = inodes.arena[working_dir.inode].read();
        if let Kind::Root { .. } = guard.deref() {
            return __WASI_EACCES;
        }
    }
    if !has_rights(working_dir.rights, __WASI_RIGHT_PATH_CREATE_DIRECTORY) {
        return __WASI_EACCES;
    }
    let path_string = unsafe { get_input_str!(memory, path, path_len) };
    debug!("=> fd: {}, path: {}", fd, &path_string);

    let path = std::path::PathBuf::from(&path_string);
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
        let mut guard = inodes.arena[cur_dir_inode].write();
        match guard.deref_mut() {
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
                    drop(guard);

                    // TODO: double check this doesn't risk breaking the sandbox
                    adjusted_path.push(comp);
                    if let Ok(adjusted_path_stat) = path_filestat_get_internal(
                        memory,
                        state,
                        inodes.deref_mut(),
                        fd,
                        0,
                        &adjusted_path.to_string_lossy(),
                    ) {
                        if adjusted_path_stat.st_filetype != __WASI_FILETYPE_DIRECTORY {
                            return __WASI_ENOTDIR;
                        }
                    } else {
                        wasi_try!(state.fs_create_dir(&adjusted_path));
                    }
                    let kind = Kind::Dir {
                        parent: Some(cur_dir_inode),
                        path: adjusted_path,
                        entries: Default::default(),
                    };
                    let new_inode = wasi_try!(state.fs.create_inode(
                        inodes.deref_mut(),
                        kind,
                        false,
                        comp.to_string()
                    ));

                    // reborrow to insert
                    {
                        let mut guard = inodes.arena[cur_dir_inode].write();
                        if let Kind::Dir {
                            ref mut entries, ..
                        } = guard.deref_mut()
                        {
                            entries.insert(comp.to_string(), new_inode);
                        }
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8>,
    path_len: u32,
    buf: WasmPtr<__wasi_filestat_t>,
) -> __wasi_errno_t {
    debug!("wasi::path_filestat_get");
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);

    let path_string = unsafe { get_input_str!(memory, path, path_len) };

    let stat = wasi_try!(path_filestat_get_internal(
        memory,
        state,
        inodes.deref_mut(),
        fd,
        flags,
        &path_string
    ));

    wasi_try_mem!(buf.deref(memory).write(stat));

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
pub fn path_filestat_get_internal(
    memory: &Memory,
    state: &WasiState,
    inodes: &mut crate::WasiInodes,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path_string: &str,
) -> Result<__wasi_filestat_t, __wasi_errno_t> {
    let root_dir = state.fs.get_fd(fd)?;

    if !has_rights(root_dir.rights, __WASI_RIGHT_PATH_FILESTAT_GET) {
        return Err(__WASI_EACCES);
    }
    debug!("=> base_fd: {}, path: {}", fd, path_string);

    let file_inode = state.fs.get_inode_at_path(
        inodes,
        fd,
        path_string,
        flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    )?;
    if inodes.arena[file_inode].is_preopened {
        Ok(inodes.arena[file_inode]
            .stat
            .read()
            .unwrap()
            .deref()
            .clone())
    } else {
        let guard = inodes.arena[file_inode].read();
        state.fs.get_stat_for_kind(inodes.deref(), guard.deref())
    }
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    flags: __wasi_lookupflags_t,
    path: WasmPtr<u8>,
    path_len: u32,
    st_atim: __wasi_timestamp_t,
    st_mtim: __wasi_timestamp_t,
    fst_flags: __wasi_fstflags_t,
) -> __wasi_errno_t {
    debug!("wasi::path_filestat_set_times");
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
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

    let path_string = unsafe { get_input_str!(memory, path, path_len) };
    debug!("=> base_fd: {}, path: {}", fd, &path_string);

    let file_inode = wasi_try!(state.fs.get_inode_at_path(
        inodes.deref_mut(),
        fd,
        &path_string,
        flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    ));
    let stat = {
        let guard = inodes.arena[file_inode].read();
        wasi_try!(state.fs.get_stat_for_kind(inodes.deref(), guard.deref()))
    };

    let inode = &inodes.arena[fd_inode];

    if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 || fst_flags & __WASI_FILESTAT_SET_ATIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_ATIM != 0 {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_atim = time_to_set;
    }
    if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 || fst_flags & __WASI_FILESTAT_SET_MTIM_NOW != 0 {
        let time_to_set = if fst_flags & __WASI_FILESTAT_SET_MTIM != 0 {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_mtim = time_to_set;
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
    thread: &WasiThread,
    old_fd: __wasi_fd_t,
    old_flags: __wasi_lookupflags_t,
    old_path: WasmPtr<u8>,
    old_path_len: u32,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8>,
    new_path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_link");
    if old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);
    let old_path_str = unsafe { get_input_str!(memory, old_path, old_path_len) };
    let new_path_str = unsafe { get_input_str!(memory, new_path, new_path_len) };
    let source_fd = wasi_try!(state.fs.get_fd(old_fd));
    let target_fd = wasi_try!(state.fs.get_fd(new_fd));
    debug!(
        "=> source_fd: {}, source_path: {}, target_fd: {}, target_path: {}",
        old_fd, &old_path_str, new_fd, new_path_str
    );

    if !(has_rights(source_fd.rights, __WASI_RIGHT_PATH_LINK_SOURCE)
        && has_rights(target_fd.rights, __WASI_RIGHT_PATH_LINK_TARGET))
    {
        return __WASI_EACCES;
    }

    let source_inode = wasi_try!(state.fs.get_inode_at_path(
        inodes.deref_mut(),
        old_fd,
        &old_path_str,
        old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    ));
    let target_path_arg = std::path::PathBuf::from(&new_path_str);
    let (target_parent_inode, new_entry_name) = wasi_try!(state.fs.get_parent_inode_at_path(
        inodes.deref_mut(),
        new_fd,
        &target_path_arg,
        false
    ));

    if inodes.arena[source_inode].stat.write().unwrap().st_nlink == __wasi_linkcount_t::max_value()
    {
        return __WASI_EMLINK;
    }
    {
        let mut guard = inodes.arena[target_parent_inode].write();
        match guard.deref_mut() {
            Kind::Dir { entries, .. } => {
                if entries.contains_key(&new_entry_name) {
                    return __WASI_EEXIST;
                }
                entries.insert(new_entry_name, source_inode);
            }
            Kind::Root { .. } => return __WASI_EINVAL,
            Kind::File { .. } | Kind::Symlink { .. } | Kind::Buffer { .. } => {
                return __WASI_ENOTDIR
            }
        }
    }
    inodes.arena[source_inode].stat.write().unwrap().st_nlink += 1;

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
    thread: &WasiThread,
    dirfd: __wasi_fd_t,
    dirflags: __wasi_lookupflags_t,
    path: WasmPtr<u8>,
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
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);
    /* TODO: find actual upper bound on name size (also this is a path, not a name :think-fish:) */
    if path_len > 1024 * 1024 {
        return __WASI_ENAMETOOLONG;
    }

    let fd_ref = fd.deref(memory);

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
    let path_string = unsafe { get_input_str!(memory, path, path_len) };

    debug!("=> fd: {}, path: {}", dirfd, &path_string);

    let path_arg = std::path::PathBuf::from(&path_string);
    let maybe_inode = state.fs.get_inode_at_path(
        inodes.deref_mut(),
        dirfd,
        &path_string,
        dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    );

    let mut open_flags = 0;
    // TODO: traverse rights of dirs properly
    // COMMENTED OUT: WASI isn't giving appropriate rights here when opening
    //              TODO: look into this; file a bug report if this is a bug
    let adjusted_rights = /*fs_rights_base &*/ working_dir_rights_inheriting;
    let mut open_options = state.fs_new_open_options();
    let inode = if let Ok(inode) = maybe_inode {
        // Happy path, we found the file we're trying to open
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::File {
                ref mut handle,
                path,
                fd,
            } => {
                if let Some(special_fd) = fd {
                    // short circuit if we're dealing with a special file
                    assert!(handle.is_some());
                    wasi_try_mem!(fd_ref.write(*special_fd));
                    return __WASI_ESUCCESS;
                }
                if o_flags & __WASI_O_DIRECTORY != 0 {
                    return __WASI_ENOTDIR;
                }
                if o_flags & __WASI_O_EXCL != 0 {
                    return __WASI_EEXIST;
                }

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
                *handle = Some(wasi_try!(open_options
                    .open(&path)
                    .map_err(fs_error_into_wasi_err)));
            }
            Kind::Buffer { .. } => unimplemented!("wasi::path_open for Buffer type files"),
            Kind::Dir { .. } | Kind::Root { .. } => {}
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
                inodes.deref_mut(),
                dirfd,
                &path_arg,
                dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0
            ));
            let new_file_host_path = {
                let guard = inodes.arena[parent_inode].read();
                match guard.deref() {
                    Kind::Dir { path, .. } => {
                        let mut new_path = path.clone();
                        new_path.push(&new_entity_name);
                        new_path
                    }
                    Kind::Root { .. } => return __WASI_EACCES,
                    _ => return __WASI_EINVAL,
                }
            };
            // once we got the data we need from the parent, we lookup the host file
            // todo: extra check that opening with write access is okay
            let handle = {
                let open_options = open_options
                    .read(true)
                    .append(fs_flags & __WASI_FDFLAG_APPEND != 0)
                    // TODO: ensure these rights are actually valid given parent, etc.
                    // write access is required for creating a file
                    .write(true)
                    .create_new(true);
                open_flags |= Fd::READ | Fd::WRITE | Fd::CREATE | Fd::TRUNCATE;

                Some(wasi_try!(open_options.open(&new_file_host_path).map_err(
                    |e| {
                        debug!("Error opening file {}", e);
                        fs_error_into_wasi_err(e)
                    }
                )))
            };

            let new_inode = {
                let kind = Kind::File {
                    handle,
                    path: new_file_host_path,
                    fd: None,
                };
                wasi_try!(state.fs.create_inode(
                    inodes.deref_mut(),
                    kind,
                    false,
                    new_entity_name.clone()
                ))
            };

            {
                let mut guard = inodes.arena[parent_inode].write();
                if let Kind::Dir {
                    ref mut entries, ..
                } = guard.deref_mut()
                {
                    entries.insert(new_entity_name, new_inode);
                }
            }

            new_inode
        } else {
            return maybe_inode.unwrap_err();
        }
    };

    {
        debug!("inode {:?} value {:#?} found!", inode, inodes.arena[inode]);
    }

    // TODO: check and reduce these
    // TODO: ensure a mutable fd to root can never be opened
    let out_fd = wasi_try!(state.fs.create_fd(
        adjusted_rights,
        fs_rights_inheriting,
        fs_flags,
        open_flags,
        inode
    ));

    wasi_try_mem!(fd_ref.write(out_fd));
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
    thread: &WasiThread,
    dir_fd: __wasi_fd_t,
    path: WasmPtr<u8>,
    path_len: u32,
    buf: WasmPtr<u8>,
    buf_len: u32,
    buf_used: WasmPtr<u32>,
) -> __wasi_errno_t {
    debug!("wasi::path_readlink");
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);

    let base_dir = wasi_try!(state.fs.get_fd(dir_fd));
    if !has_rights(base_dir.rights, __WASI_RIGHT_PATH_READLINK) {
        return __WASI_EACCES;
    }
    let path_str = unsafe { get_input_str!(memory, path, path_len) };
    let inode = wasi_try!(state
        .fs
        .get_inode_at_path(inodes.deref_mut(), dir_fd, &path_str, false));

    {
        let guard = inodes.arena[inode].read();
        if let Kind::Symlink { relative_path, .. } = guard.deref() {
            let rel_path_str = relative_path.to_string_lossy();
            debug!("Result => {:?}", rel_path_str);
            let bytes = rel_path_str.bytes();
            if bytes.len() >= buf_len as usize {
                return __WASI_EOVERFLOW;
            }
            let bytes: Vec<_> = bytes.collect();

            let out = wasi_try_mem!(buf.slice(memory, bytes.len() as u32));
            wasi_try_mem!(out.write_slice(&bytes[..]));
            // should we null terminate this?

            wasi_try_mem!(buf_used.deref(memory).write(bytes.len() as u32));
        } else {
            return __WASI_EINVAL;
        }
    }

    __WASI_ESUCCESS
}

/// Returns __WASI_ENOTEMTPY if directory is not empty
pub fn path_remove_directory(
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8>,
    path_len: u32,
) -> __wasi_errno_t {
    // TODO check if fd is a dir, ensure it's within sandbox, etc.
    debug!("wasi::path_remove_directory");
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    let path_str = unsafe { get_input_str!(memory, path, path_len) };

    let inode = wasi_try!(state
        .fs
        .get_inode_at_path(inodes.deref_mut(), fd, &path_str, false));
    let (parent_inode, childs_name) = wasi_try!(state.fs.get_parent_inode_at_path(
        inodes.deref_mut(),
        fd,
        std::path::Path::new(&path_str),
        false
    ));

    let host_path_to_remove = {
        let guard = inodes.arena[inode].read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if !entries.is_empty() || wasi_try!(state.fs_read_dir(path)).count() != 0 {
                    return __WASI_ENOTEMPTY;
                }
                path.clone()
            }
            Kind::Root { .. } => return __WASI_EACCES,
            _ => return __WASI_ENOTDIR,
        }
    };

    {
        let mut guard = inodes.arena[parent_inode].write();
        match guard.deref_mut() {
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
    }

    if let Err(err) = state.fs_remove_dir(host_path_to_remove) {
        // reinsert to prevent FS from being in bad state
        let mut guard = inodes.arena[parent_inode].write();
        if let Kind::Dir {
            ref mut entries, ..
        } = guard.deref_mut()
        {
            entries.insert(childs_name, inode);
        }
        return err;
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
    thread: &WasiThread,
    old_fd: __wasi_fd_t,
    old_path: WasmPtr<u8>,
    old_path_len: u32,
    new_fd: __wasi_fd_t,
    new_path: WasmPtr<u8>,
    new_path_len: u32,
) -> __wasi_errno_t {
    debug!(
        "wasi::path_rename: old_fd = {}, new_fd = {}",
        old_fd, new_fd
    );
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);
    let source_str = unsafe { get_input_str!(memory, old_path, old_path_len) };
    let source_path = std::path::Path::new(&source_str);
    let target_str = unsafe { get_input_str!(memory, new_path, new_path_len) };
    let target_path = std::path::Path::new(&target_str);
    debug!("=> rename from {} to {}", &source_str, &target_str);

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
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), old_fd, source_path, true));
    let (target_parent_inode, target_entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), new_fd, target_path, true));

    let host_adjusted_target_path = {
        let guard = inodes.arena[target_parent_inode].read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if entries.contains_key(&target_entry_name) {
                    return __WASI_EEXIST;
                }
                let mut out_path = path.clone();
                out_path.push(std::path::Path::new(&target_entry_name));
                out_path
            }
            Kind::Root { .. } => return __WASI_ENOTCAPABLE,
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                unreachable!("Fatal internal logic error: parent of inode is not a directory")
            }
        }
    };

    let source_entry = {
        let mut guard = inodes.arena[source_parent_inode].write();
        match guard.deref_mut() {
            Kind::Dir { entries, .. } => {
                wasi_try!(entries.remove(&source_entry_name).ok_or(__WASI_ENOENT))
            }
            Kind::Root { .. } => return __WASI_ENOTCAPABLE,
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                unreachable!("Fatal internal logic error: parent of inode is not a directory")
            }
        }
    };

    {
        let mut guard = inodes.arena[source_entry].write();
        match guard.deref_mut() {
            Kind::File {
                handle, ref path, ..
            } => {
                // TODO: investigate why handle is not always there, it probably should be.
                // My best guess is the fact that a handle means currently open and a path
                // just means reference to host file on disk. But ideally those concepts
                // could just be unified even if there's a `Box<dyn VirtualFile>` which just
                // implements the logic of "I'm not actually a file, I'll try to be as needed".
                let result = if let Some(h) = handle {
                    drop(guard);
                    state.fs_rename(&source_path, &host_adjusted_target_path)
                } else {
                    let path_clone = path.clone();
                    drop(guard);
                    let out = state.fs_rename(&path_clone, &host_adjusted_target_path);
                    {
                        let mut guard = inodes.arena[source_entry].write();
                        if let Kind::File { ref mut path, .. } = guard.deref_mut() {
                            *path = host_adjusted_target_path;
                        } else {
                            unreachable!()
                        }
                    }
                    out
                };
                // if the above operation failed we have to revert the previous change and then fail
                if let Err(e) = result.clone() {
                    let mut guard = inodes.arena[source_parent_inode].write();
                    if let Kind::Dir { entries, .. } = guard.deref_mut() {
                        entries.insert(source_entry_name, source_entry);
                        return e;
                    }
                }
            }
            Kind::Dir { ref path, .. } => {
                let cloned_path = path.clone();
                if let Err(e) = state.fs_rename(cloned_path, &host_adjusted_target_path) {
                    return e;
                }
                {
                    drop(guard);
                    let mut guard = inodes.arena[source_entry].write();
                    if let Kind::Dir { path, .. } = guard.deref_mut() {
                        *path = host_adjusted_target_path;
                    }
                }
            }
            Kind::Buffer { .. } => {}
            Kind::Symlink { .. } => {}
            Kind::Root { .. } => unreachable!("The root can not be moved"),
        }
    }

    {
        let mut guard = inodes.arena[target_parent_inode].write();
        if let Kind::Dir { entries, .. } = guard.deref_mut() {
            let result = entries.insert(target_entry_name, source_entry);
            assert!(
                result.is_none(),
                "Fatal error: race condition on filesystem detected or internal logic error"
            );
        }
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
    thread: &WasiThread,
    old_path: WasmPtr<u8>,
    old_path_len: u32,
    fd: __wasi_fd_t,
    new_path: WasmPtr<u8>,
    new_path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_symlink");
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);
    let old_path_str = unsafe { get_input_str!(memory, old_path, old_path_len) };
    let new_path_str = unsafe { get_input_str!(memory, new_path, new_path_len) };
    let base_fd = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(base_fd.rights, __WASI_RIGHT_PATH_SYMLINK) {
        return __WASI_EACCES;
    }

    // get the depth of the parent + 1 (UNDER INVESTIGATION HMMMMMMMM THINK FISH ^ THINK FISH)
    let old_path_path = std::path::Path::new(&old_path_str);
    let (source_inode, _) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), fd, old_path_path, true));
    let depth = wasi_try!(state
        .fs
        .path_depth_from_fd(inodes.deref(), fd, source_inode))
        - 1;

    let new_path_path = std::path::Path::new(&new_path_str);
    let (target_parent_inode, entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), fd, new_path_path, true));

    // short circuit if anything is wrong, before we create an inode
    {
        let guard = inodes.arena[target_parent_inode].read();
        match guard.deref() {
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
    }

    let mut source_path = std::path::Path::new(&old_path_str);
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
    let new_inode = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind,
        false,
        entry_name.clone(),
    );

    {
        let mut guard = inodes.arena[target_parent_inode].write();
        if let Kind::Dir {
            ref mut entries, ..
        } = guard.deref_mut()
        {
            entries.insert(entry_name, new_inode);
        }
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
    thread: &WasiThread,
    fd: __wasi_fd_t,
    path: WasmPtr<u8>,
    path_len: u32,
) -> __wasi_errno_t {
    debug!("wasi::path_unlink_file");
    let (memory, state, mut inodes) = thread.get_memory_and_wasi_state_and_inodes_mut(0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    if !has_rights(base_dir.rights, __WASI_RIGHT_PATH_UNLINK_FILE) {
        return __WASI_EACCES;
    }
    let path_str = unsafe { get_input_str!(memory, path, path_len) };
    debug!("Requested file: {}", path_str);

    let inode = wasi_try!(state
        .fs
        .get_inode_at_path(inodes.deref_mut(), fd, &path_str, false));
    let (parent_inode, childs_name) = wasi_try!(state.fs.get_parent_inode_at_path(
        inodes.deref_mut(),
        fd,
        std::path::Path::new(&path_str),
        false
    ));

    let removed_inode = {
        let mut guard = inodes.arena[parent_inode].write();
        match guard.deref_mut() {
            Kind::Dir {
                ref mut entries, ..
            } => {
                let removed_inode = wasi_try!(entries.remove(&childs_name).ok_or(__WASI_EINVAL));
                // TODO: make this a debug assert in the future
                assert!(inode == removed_inode);
                debug_assert!(inodes.arena[inode].stat.read().unwrap().st_nlink > 0);
                removed_inode
            }
            Kind::Root { .. } => return __WASI_EACCES,
            _ => unreachable!(
                "Internal logic error in wasi::path_unlink_file, parent is not a directory"
            ),
        }
    };

    let st_nlink = {
        let mut guard = inodes.arena[removed_inode].stat.write().unwrap();
        guard.st_nlink -= 1;
        guard.st_nlink
    };
    if st_nlink == 0 {
        {
            let mut guard = inodes.arena[removed_inode].write();
            match guard.deref_mut() {
                Kind::File { handle, path, .. } => {
                    if let Some(h) = handle {
                        wasi_try!(h.unlink().map_err(fs_error_into_wasi_err));
                    } else {
                        // File is closed
                        // problem with the abstraction, we can't call unlink because there's no handle
                        // drop mutable borrow on `path`
                        let path = path.clone();
                        wasi_try!(state.fs_remove_file(path));
                    }
                }
                Kind::Dir { .. } | Kind::Root { .. } => return __WASI_EISDIR,
                Kind::Symlink { .. } => {
                    // TODO: actually delete real symlinks and do nothing for virtual symlinks
                }
                _ => unimplemented!("wasi::path_unlink_file for Buffer"),
            }
        }
        // TODO: test this on Windows and actually make it portable
        // make the file an orphan fd if the fd is still open
        let fd_is_orphaned = {
            let guard = inodes.arena[removed_inode].read();
            if let Kind::File { handle, .. } = guard.deref() {
                handle.is_some()
            } else {
                false
            }
        };
        let removed_inode_val = unsafe { state.fs.remove_inode(inodes.deref_mut(), removed_inode) };
        assert!(
            removed_inode_val.is_some(),
            "Inode could not be removed because it doesn't exist"
        );

        if fd_is_orphaned {
            inodes
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
    thread: &WasiThread,
    in_: WasmPtr<__wasi_subscription_t>,
    out_: WasmPtr<__wasi_event_t>,
    nsubscriptions: u32,
    nevents: WasmPtr<u32>,
) -> Result<__wasi_errno_t, WasiError> {
    trace!("wasi::poll_oneoff");
    trace!("  => nsubscriptions = {}", nsubscriptions);
    let (memory, state, inodes) = thread.get_memory_and_wasi_state_and_inodes(0);

    let subscription_array = wasi_try_mem_ok!(in_.slice(memory, nsubscriptions));
    let event_array = wasi_try_mem_ok!(out_.slice(memory, nsubscriptions));
    let mut events_seen = 0;
    let out_ptr = nevents.deref(memory);

    let mut fd_guards = vec![];
    let mut clock_subs = vec![];
    let mut in_events = vec![];
    let mut time_to_sleep = Duration::from_millis(5);

    for sub in subscription_array.iter() {
        let s: WasiSubscription = wasi_try_ok!(wasi_try_mem_ok!(sub.read()).try_into());
        let mut peb = PollEventBuilder::new();
        
        let fd = match s.event_type {
            EventType::Read(__wasi_subscription_fs_readwrite_t { fd }) => {
                match fd {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    _ => {
                        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd), thread);
                        if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_READ) {
                            return Ok(__WASI_EACCES);
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
                        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd), thread);
                        if !has_rights(fd_entry.rights, __WASI_RIGHT_FD_WRITE) {
                            return Ok(__WASI_EACCES);
                        }
                    }
                }
                in_events.push(peb.add(PollEvent::PollOut).build());
                Some(fd)
            }
            EventType::Clock(clock_info) => {
                if clock_info.clock_id == __WASI_CLOCK_REALTIME
                    || clock_info.clock_id == __WASI_CLOCK_MONOTONIC
                {
                    // this is a hack
                    // TODO: do this properly
                    time_to_sleep = Duration::from_nanos(clock_info.timeout);
                    clock_subs.push((clock_info, s.user_data));
                    None
                } else {
                    unimplemented!("Polling not implemented for clocks yet");
                }
            }
        };

        if let Some(fd) = fd {
            let wasi_file_ref = match fd {
                __WASI_STDERR_FILENO => {
                    wasi_try_ok!(
                        inodes
                            .stderr(&state.fs.fd_map)
                            .map_err(fs_error_into_wasi_err),
                        thread
                    )
                }
                __WASI_STDIN_FILENO => {
                    wasi_try_ok!(
                        inodes
                            .stdin(&state.fs.fd_map)
                            .map_err(fs_error_into_wasi_err),
                        thread
                    )
                }
                __WASI_STDOUT_FILENO => {
                    wasi_try_ok!(
                        inodes
                            .stdout(&state.fs.fd_map)
                            .map_err(fs_error_into_wasi_err),
                        thread
                    )
                }
                _ => {
                    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd), thread);
                    let inode = fd_entry.inode;
                    if !has_rights(fd_entry.rights, __WASI_RIGHT_POLL_FD_READWRITE) {
                        return Ok(__WASI_EACCES);
                    }

                    {
                        let guard = inodes.arena[inode].read();
                        match guard.deref() {
                            Kind::File { handle, .. } => {
                                if let Some(h) = handle {
                                    crate::state::InodeValFileReadGuard { guard }
                                } else {
                                    return Ok(__WASI_EBADF);
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
                }
            };
            fd_guards.push(wasi_file_ref);
        }
    }

    let fds = {
        let mut f = vec![];
        for fd in fd_guards.iter() {
            f.push(wasi_try_ok!(fd.as_ref().ok_or(__WASI_EBADF)).deref());
        }
        f
    };

    let mut seen_events = vec![Default::default(); in_events.len()];

    let start = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
    let mut triggered = 0;
    while triggered == 0 {
        let now = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
        let delta = match now.checked_sub(start) {
            Some(a) => Duration::from_nanos(a as u64),
            None => Duration::ZERO
        };
        match poll(
            fds.as_slice(),
            in_events.as_slice(),
            seen_events.as_mut_slice(),
            Duration::from_millis(1),
        )
        {
            Ok(0) => {
                thread.yield_now()?;
            },
            Ok(a) => {
                triggered = a;
            },
            Err(FsError::WouldBlock) => {
                thread.sleep(Duration::from_millis(1))?;
            },
            Err(err) => {
                return Ok(fs_error_into_wasi_err(err));
            }
        };
        if delta > time_to_sleep {
            break;
        }
    }

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
                    bytes_available = wasi_try_ok!(
                        fds[i]
                            .bytes_available_read()
                            .map_err(fs_error_into_wasi_err),
                        thread
                    )
                    .unwrap_or(0usize);
                    error = __WASI_ESUCCESS;
                }
                PollEvent::PollOut => {
                    bytes_available = wasi_try_ok!(
                        fds[i]
                            .bytes_available_write()
                            .map_err(fs_error_into_wasi_err),
                        thread
                    )
                    .unwrap_or(0usize);
                    error = __WASI_ESUCCESS;
                }
            }
        }
        let event = __wasi_event_t {
            userdata: wasi_try_mem_ok!(subscription_array.index(i as u64).read()).userdata,
            error,
            type_: wasi_try_mem_ok!(subscription_array.index(i as u64).read()).type_,
            u: unsafe {
                __wasi_event_u {
                    fd_readwrite: __wasi_event_fd_readwrite_t {
                        nbytes: bytes_available as u64,
                        flags,
                    },
                }
            },
        };
        wasi_try_mem_ok!(event_array.index(events_seen as u64).write(event));
        events_seen += 1;
    }
    if triggered <= 0 {
        for (clock_info, userdata) in clock_subs {
            let event = __wasi_event_t {
                userdata,
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
            wasi_try_mem_ok!(event_array.index(events_seen as u64).write(event));
            events_seen += 1;
        }
    }
    wasi_try_mem_ok!(out_ptr.write(events_seen as u32));
    Ok(__WASI_ESUCCESS)
}

pub fn proc_exit(thread: &WasiThread, code: __wasi_exitcode_t) -> Result<(), WasiError> {
    debug!("wasi::proc_exit, {}", code);
    Err(WasiError::Exit(code))
}

pub fn proc_raise(thread: &WasiThread, sig: __wasi_signal_t) -> __wasi_errno_t {
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
pub fn random_get(thread: &WasiThread, buf: u32, buf_len: u32) -> __wasi_errno_t {
    trace!("wasi::random_get buf_len: {}", buf_len);
    let memory = thread.memory();
    let mut u8_buffer = vec![0; buf_len as usize];
    let res = getrandom::getrandom(&mut u8_buffer);
    match res {
        Ok(()) => {
            let buf = wasi_try_mem!(WasmPtr::<u8>::new(buf).slice(memory, buf_len));
            wasi_try_mem!(buf.write_slice(&u8_buffer));
            __WASI_ESUCCESS
        }
        Err(_) => __WASI_EIO,
    }
}

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(thread: &WasiThread) -> Result<__wasi_errno_t, WasiError> {
    trace!("wasi::sched_yield");
    thread.yield_now()?;
    Ok(__WASI_ESUCCESS)
}

pub fn sock_recv(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    ri_data: WasmPtr<__wasi_iovec_t>,
    ri_data_len: u32,
    ri_flags: __wasi_riflags_t,
    ro_datalen: WasmPtr<u32>,
    ro_flags: WasmPtr<__wasi_roflags_t>,
) -> __wasi_errno_t {
    trace!("wasi::sock_recv");
    unimplemented!("wasi::sock_recv")
}
pub fn sock_send(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    si_data: WasmPtr<__wasi_ciovec_t>,
    si_data_len: u32,
    si_flags: __wasi_siflags_t,
    so_datalen: WasmPtr<u32>,
) -> __wasi_errno_t {
    trace!("wasi::sock_send");
    unimplemented!("wasi::sock_send")
}
pub fn sock_shutdown(
    thread: &WasiThread,
    sock: __wasi_fd_t,
    how: __wasi_sdflags_t,
) -> __wasi_errno_t {
    trace!("wasi::sock_shutdown");
    unimplemented!("wasi::sock_shutdown")
}
