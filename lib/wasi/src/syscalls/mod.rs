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
pub mod proxy;
pub mod native;
pub mod utils;

use self::types::*;
use self::utils::*;
use crate::{
    ptr::{Array, WasmPtr},
    state::{
        self, fs_error_into_wasi_err, iterate_poll_events, poll,
        virtual_file_type_to_wasi_file_type, Fd, Inode, InodeVal, Kind, PollEvent,
        PollEventBuilder, WasiState, MAX_SYMLINKS,
    },
    WasiEnv, WasiError,
};
use std::borrow::Borrow;
use std::convert::{Infallible, TryInto};
use std::io::{self, Read, Seek, Write};
use tracing::{debug, trace};
use wasmer::{Memory, RuntimeError, Value, WasmCell};
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

/// ### `args_get()`
/// Read command-line argument data.
/// The sizes of the buffers should match that returned by [`args_sizes_get()`](#args_sizes_get).
/// Inputs:
/// - `char **argv`
///     A pointer to a buffer to write the argument pointers.
/// - `char *argv_buf`
///     A pointer to a buffer to write the argument string data.
///
pub fn args_get(env: &WasiEnv, argv: WasmPtr<WasmPtr<u8, Array>, Array>, argv_buf: WasmPtr<u8, Array>) -> __wasi_errno_t {
    debug!("wasi::args_get");
    env.proxy.args_get(env, argv, argv_buf)
}

/// ### `args_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *argc`
///     The number of arguments.
/// - `size_t *argv_buf_size`
///     The size of the argument string data.
pub fn args_sizes_get(env: &WasiEnv, argc: WasmPtr<u32>, argv_buf_size: WasmPtr<u32>) -> __wasi_errno_t {
    debug!("wasi::args_sizes_get");
    env.proxy.args_sizes_get(env, argc, argv_buf_size)
}

pub fn clock_res_get(env: &WasiEnv, clock_id: __wasi_clockid_t, resolution_ptr: WasmPtr<__wasi_timestamp_t>) -> __wasi_errno_t {
    debug!("wasi::clock_res_get");
    let memory = env.memory();
    match env.proxy.clock_res_get(env, clock_id) {
        Ok(time) => {
            let resolution_ptr = wasi_try!(resolution_ptr.deref(memory));
            resolution_ptr.set(time);
            __WASI_ESUCCESS
        },
        Err(err) => err
    }
}

/// ### `clock_res_get()`
/// Get the resolution of the specified clock
/// Input:
/// - `__wasi_clockid_t clock_id`
///     The ID of the clock to get the resolution of
/// Output:
/// - `__wasi_timestamp_t *resolution`
///     The resolution of the clock in nanoseconds
pub fn clock_time_get(env: &WasiEnv, clock_id: __wasi_clockid_t, precision: __wasi_timestamp_t, time_ptr: WasmPtr<__wasi_timestamp_t>) -> __wasi_errno_t {
    trace!(
        "wasi::clock_time_get clock_id: {}, precision: {}",
        clock_id, precision
    );
    let memory = env.memory();
    match env.proxy.clock_time_get(env, clock_id, precision) {
        Ok(time) => {
            let time_ptr = wasi_try!(time_ptr.deref(memory));
            time_ptr.set(time);
            __WASI_ESUCCESS
        },
        Err(err) => err
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
pub fn environ_get(env: &WasiEnv, environ: WasmPtr<WasmPtr<u8, Array>, Array>, environ_buf: WasmPtr<u8, Array>) -> __wasi_errno_t {
    debug!(
        "wasi::environ_get. Environ: {:?}, environ_buf: {:?}",
        environ, environ_buf
    );
    env.proxy.environ_get(env, environ, environ_buf)
}

/// ### `environ_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *environ_count`
///     The number of environment variables.
/// - `size_t *environ_buf_size`
///     The size of the environment variable string data.
pub fn environ_sizes_get(env: &WasiEnv, environ_count: WasmPtr<u32>, environ_buf_size: WasmPtr<u32>) -> __wasi_errno_t {
    debug!("wasi::environ_sizes_get");
    env.proxy.environ_sizes_get(env, environ_count, environ_buf_size)
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
pub fn fd_advise(env: &WasiEnv, fd: __wasi_fd_t, offset: __wasi_filesize_t, len: __wasi_filesize_t, advice: __wasi_advice_t) -> __wasi_errno_t {
    debug!("wasi::fd_advise: fd={}", fd);
    env.proxy.fd_advise(env, fd, offset, len, advice)
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
pub fn fd_allocate(env: &WasiEnv, fd: __wasi_fd_t, offset: __wasi_filesize_t, len: __wasi_filesize_t) -> __wasi_errno_t {
    debug!("wasi::fd_allocate");
    env.proxy.fd_allocate(env, fd, offset, len)
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
pub fn fd_close(env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_close: fd={}", fd);
    env.proxy.fd_close(env, fd)
}

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to sync
pub fn fd_datasync(env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_datasync");
    env.proxy.fd_datasync(env, fd)
}

/// ### `fd_fdstat_get()`
/// Get metadata of a file descriptor
/// Input:
/// - `__wasi_fd_t fd`
///     The file descriptor whose metadata will be accessed
/// Output:
/// - `__wasi_fdstat_t *buf`
///     The location where the metadata will be written
pub fn fd_fdstat_get(env: &WasiEnv, fd: __wasi_fd_t, buf_ptr: WasmPtr<__wasi_fdstat_t>) -> __wasi_errno_t {
    debug!(
        "wasi::fd_fdstat_get: fd={}, buf_ptr={}",
        fd,
        buf_ptr.offset()
    );
    env.proxy.fd_fdstat_get(env, fd, buf_ptr)
}

/// ### `fd_fdstat_set_flags()`
/// Set file descriptor flags for a file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to apply the new flags to
/// - `__wasi_fdflags_t flags`
///     The flags to apply to `fd`
pub fn fd_fdstat_set_flags(env: &WasiEnv, fd: __wasi_fd_t, flags: __wasi_fdflags_t) -> __wasi_errno_t {
    debug!("wasi::fd_fdstat_set_flags");
    env.proxy.fd_fdstat_set_flags(env, fd, flags)
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
pub fn fd_fdstat_set_rights(env: &WasiEnv, fd: __wasi_fd_t, fs_rights_base: __wasi_rights_t, fs_rights_inheriting: __wasi_rights_t) -> __wasi_errno_t {
    debug!("wasi::fd_fdstat_set_rights");
    env.proxy.fd_fdstat_set_rights(env, fd, fs_rights_base, fs_rights_inheriting)
}

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `__wasi_fd_t fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `__wasi_filestat_t *buf`
///     Where the metadata from `fd` will be written
pub fn fd_filestat_get(env: &WasiEnv, fd: __wasi_fd_t, buf: WasmPtr<__wasi_filestat_t>) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_get");
    env.proxy.fd_filestat_get(env, fd, buf)
}

/// ### `fd_filestat_set_size()`
/// Change the size of an open file, zeroing out any new bytes
/// Inputs:
/// - `__wasi_fd_t fd`
///     File descriptor to adjust
/// - `__wasi_filesize_t st_size`
///     New size that `fd` will be set to
pub fn fd_filestat_set_size(env: &WasiEnv, fd: __wasi_fd_t, st_size: __wasi_filesize_t) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_set_size");
    env.proxy.fd_filestat_set_size(env, fd, st_size)
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
pub fn fd_filestat_set_times(env: &WasiEnv, fd: __wasi_fd_t, st_atim: __wasi_timestamp_t, st_mtim: __wasi_timestamp_t, fst_flags: __wasi_fstflags_t) -> __wasi_errno_t {
    debug!("wasi::fd_filestat_set_times");
    env.proxy.fd_filestat_set_times(env, fd, st_atim, st_mtim, fst_flags)
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
pub fn fd_pread(env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_iovec_t, Array>, iovs_len: u32, offset: __wasi_filesize_t, nread: WasmPtr<u32>) -> __wasi_errno_t {
    debug!("wasi::fd_pread: fd={}, offset={}", fd, offset);
    env.proxy.fd_pread(env, fd, iovs, iovs_len, offset, nread)
}

/// ### `fd_prestat_get()`
/// Get metadata about a preopened file descriptor
/// Input:
/// - `__wasi_fd_t fd`
///     The preopened file descriptor to query
/// Output:
/// - `__wasi_prestat *buf`
///     Where the metadata will be written
pub fn fd_prestat_get(env: &WasiEnv, fd: __wasi_fd_t, buf: WasmPtr<__wasi_prestat_t>) -> __wasi_errno_t {
    debug!("wasi::fd_prestat_get: fd={}", fd);
    env.proxy.fd_prestat_get(env, fd, buf)
}

pub fn fd_prestat_dir_name(env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
    debug!(
        "wasi::fd_prestat_dir_name: fd={}, path_len={}",
        fd, path_len
    );
    env.proxy.fd_prestat_dir_name(env, fd, path, path_len)
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
pub fn fd_pwrite(env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_ciovec_t, Array>, iovs_len: u32, offset: __wasi_filesize_t, nwritten: WasmPtr<u32>) -> __wasi_errno_t {
    debug!("wasi::fd_pwrite");
    env.proxy.fd_pwrite(env, fd, iovs, iovs_len, offset, nwritten)
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
pub fn fd_read(env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_iovec_t, Array>, iovs_len: u32, nread: WasmPtr<u32>) -> __wasi_errno_t {
    trace!("wasi::fd_read: fd={}", fd);
    env.proxy.fd_read(env, fd, iovs, iovs_len, nread)
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
pub fn fd_readdir(env: &WasiEnv, fd: __wasi_fd_t, buf: WasmPtr<u8, Array>, buf_len: u32, cookie: __wasi_dircookie_t, bufused: WasmPtr<u32>) -> __wasi_errno_t {
    debug!("wasi::fd_readdir");
    env.proxy.fd_readdir(env, fd, buf, buf_len, cookie, bufused)
}

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `__wasi_fd_t from`
///     File descriptor to copy
/// - `__wasi_fd_t to`
///     Location to copy file descriptor to
pub fn fd_renumber(env: &WasiEnv, from: __wasi_fd_t, to: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_renumber: from={}, to={}", from, to);
    env.proxy.fd_renumber(env, from, to)
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
pub fn fd_seek(env: &WasiEnv, fd: __wasi_fd_t, offset: __wasi_filedelta_t, whence: __wasi_whence_t, newoffset: WasmPtr<__wasi_filesize_t>) -> __wasi_errno_t {
    debug!("wasi::fd_seek: fd={}, offset={}", fd, offset);
    env.proxy.fd_seek(env, fd, offset, whence, newoffset)
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
pub fn fd_sync(env: &WasiEnv, fd: __wasi_fd_t) -> __wasi_errno_t {
    debug!("wasi::fd_sync");
    debug!("=> fd={}", fd);
    env.proxy.fd_sync(env, fd)
}

/// ### `fd_tell()`
/// Get the offset of the file descriptor
/// Inputs:
/// - `__wasi_fd_t fd`
///     The file descriptor to access
/// Output:
/// - `__wasi_filesize_t *offset`
///     The offset of `fd` relative to the start of the file
pub fn fd_tell(env: &WasiEnv, fd: __wasi_fd_t, offset: WasmPtr<__wasi_filesize_t>) -> __wasi_errno_t {
    debug!("wasi::fd_tell");
    env.proxy.fd_tell(env, fd, offset)
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
pub fn fd_write(env: &WasiEnv, fd: __wasi_fd_t, iovs: WasmPtr<__wasi_ciovec_t, Array>, iovs_len: u32, nwritten: WasmPtr<u32>) -> __wasi_errno_t {
    // If we are writing to stdout or stderr
    // we skip debug to not pollute the stdout/err
    // and do debugging happily after :)
    if fd != __WASI_STDOUT_FILENO && fd != __WASI_STDERR_FILENO {
        debug!("wasi::fd_write: fd={}", fd);
    } else {
        trace!("wasi::fd_write: fd={}", fd);
    }
    env.proxy.fd_write(env, fd, iovs, iovs_len, nwritten)
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
pub fn path_create_directory(env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
    debug!("wasi::path_create_directory");
    env.proxy.path_create_directory(env, fd, path, path_len)
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
pub fn path_filestat_get(env: &WasiEnv, fd: __wasi_fd_t, flags: __wasi_lookupflags_t, path: WasmPtr<u8, Array>, path_len: u32, buf: WasmPtr<__wasi_filestat_t>) -> __wasi_errno_t {
    debug!("wasi::path_filestat_get");
    env.proxy.path_filestat_get(env, fd, flags, path, path_len, buf)
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
pub fn path_filestat_set_times(env: &WasiEnv, fd: __wasi_fd_t, flags: __wasi_lookupflags_t, path: WasmPtr<u8, Array>, path_len: u32, st_atim: __wasi_timestamp_t, st_mtim: __wasi_timestamp_t, fst_flags: __wasi_fstflags_t) -> __wasi_errno_t {
    debug!("wasi::path_filestat_set_times");
    env.proxy.path_filestat_set_times(env, fd, flags, path, path_len, st_atim, st_mtim, fst_flags)
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
pub fn path_link(env: &WasiEnv, old_fd: __wasi_fd_t, old_flags: __wasi_lookupflags_t, old_path: WasmPtr<u8, Array>, old_path_len: u32, new_fd: __wasi_fd_t, new_path: WasmPtr<u8, Array>, new_path_len: u32) -> __wasi_errno_t {
    debug!("wasi::path_link");
    if old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    env.proxy.path_link(env, old_fd, old_flags, old_path, old_path_len, new_fd, new_path, new_path_len)
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
pub fn path_open(env: &WasiEnv, dirfd: __wasi_fd_t, dirflags: __wasi_lookupflags_t, path: WasmPtr<u8, Array>, path_len: u32, o_flags: __wasi_oflags_t, fs_rights_base: __wasi_rights_t, fs_rights_inheriting: __wasi_rights_t, fs_flags: __wasi_fdflags_t, fd: WasmPtr<__wasi_fd_t>) -> __wasi_errno_t {
    debug!("wasi::path_open");
    if dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    env.proxy.path_open(env, dirfd, dirflags, path, path_len, o_flags, fs_rights_base, fs_rights_inheriting, fs_flags, fd)
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
pub fn path_readlink(env: &WasiEnv, dir_fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32, buf: WasmPtr<u8, Array>, buf_len: u32, buf_used: WasmPtr<u32>) -> __wasi_errno_t {
    debug!("wasi::path_readlink");
    env.proxy.path_readlink(env, dir_fd, path, path_len, buf, buf_len, buf_used)
}

/// Returns __WASI_ENOTEMTPY if directory is not empty
pub fn path_remove_directory(env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
    debug!("wasi::path_remove_directory");
    env.proxy.path_remove_directory(env, fd, path, path_len)
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
pub fn path_rename(env: &WasiEnv, old_fd: __wasi_fd_t, old_path: WasmPtr<u8, Array>, old_path_len: u32, new_fd: __wasi_fd_t, new_path: WasmPtr<u8, Array>, new_path_len: u32) -> __wasi_errno_t {
    debug!(
        "wasi::path_rename: old_fd = {}, new_fd = {}",
        old_fd, new_fd
    );
    env.proxy.path_rename(env, old_fd, old_path, old_path_len, new_fd, new_path, new_path_len)
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
pub fn path_symlink(env: &WasiEnv, old_path: WasmPtr<u8, Array>, old_path_len: u32, fd: __wasi_fd_t, new_path: WasmPtr<u8, Array>, new_path_len: u32) -> __wasi_errno_t {
    debug!("wasi::path_symlink");
    env.proxy.path_symlink(env, old_path, old_path_len, fd, new_path, new_path_len)
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
pub fn path_unlink_file(env: &WasiEnv, fd: __wasi_fd_t, path: WasmPtr<u8, Array>, path_len: u32) -> __wasi_errno_t {
    debug!("wasi::path_unlink_file");
    env.proxy.path_unlink_file(env, fd, path, path_len)
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
pub fn poll_oneoff(env: &WasiEnv, in_: WasmPtr<__wasi_subscription_t, Array>, out_: WasmPtr<__wasi_event_t, Array>, nsubscriptions: u32, nevents: WasmPtr<u32>) -> __wasi_errno_t {
    trace!("wasi::poll_oneoff");
    trace!("  => nsubscriptions = {}", nsubscriptions);
    env.proxy.poll_oneoff(env, in_, out_, nsubscriptions, nevents)
}

pub fn proc_exit(env: &WasiEnv, code: __wasi_exitcode_t) {
    debug!("wasi::proc_exit, {}", code);
    env.proxy.proc_exit(env, code)
}

pub fn proc_raise(env: &WasiEnv, sig: __wasi_signal_t) -> __wasi_errno_t {
    debug!("wasi::proc_raise");
    env.proxy.proc_raise(env, sig)
}

/// ### `random_get()`
/// Fill buffer with high-quality random data.  This function may be slow and block
/// Inputs:
/// - `void *buf`
///     A pointer to a buffer where the random bytes will be written
/// - `size_t buf_len`
///     The number of bytes that will be written
pub fn random_get(env: &WasiEnv, buf: u32, buf_len: u32) -> __wasi_errno_t {
    debug!("wasi::random_get buf_len: {}", buf_len);
    env.proxy.random_get(env, buf, buf_len)
}

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(env: &WasiEnv) -> __wasi_errno_t {
    trace!("wasi::sched_yield");
    env.proxy.sched_yield(env)
}

pub fn sock_recv(env: &WasiEnv, sock: __wasi_fd_t, ri_data: WasmPtr<__wasi_iovec_t, Array>, ri_data_len: u32, ri_flags: __wasi_riflags_t, ro_datalen: WasmPtr<u32>, ro_flags: WasmPtr<__wasi_roflags_t>) -> __wasi_errno_t {
    debug!("wasi::sock_recv");
    env.proxy.sock_recv(env, sock, ri_data, ri_data_len, ri_flags, ro_datalen, ro_flags)
}

pub fn sock_send(env: &WasiEnv, sock: __wasi_fd_t, si_data: WasmPtr<__wasi_ciovec_t, Array>, si_data_len: u32, si_flags: __wasi_siflags_t, so_datalen: WasmPtr<u32>) -> __wasi_errno_t {
    debug!("wasi::sock_send");
    env.proxy.sock_send(env, sock, si_data, si_data_len, si_flags, so_datalen)
}

pub fn sock_shutdown(env: &WasiEnv, sock: __wasi_fd_t, how: __wasi_sdflags_t) -> __wasi_errno_t {
    debug!("wasi::sock_shutdown");
    env.proxy.sock_shutdown(env, sock, how)
}
