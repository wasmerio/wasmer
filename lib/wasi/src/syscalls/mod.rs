#![allow(unused, clippy::too_many_arguments, clippy::cognitive_complexity)]

pub mod types {
    pub use wasmer_wasi_types::types::*;
    pub use wasmer_wasi_types::wasi;
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
//pub mod wasi;
#[cfg(feature = "wasix")]
pub mod wasix32;
#[cfg(feature = "wasix")]
pub mod wasix64;

use self::types::{
    wasi::{
        Addressfamily, Advice, Bid, BusDataFormat, BusErrno, BusHandles, Cid, Clockid, Dircookie,
        Dirent, Errno, Event, EventEnum, EventFdReadwrite, Eventrwflags, Eventtype, Fd as WasiFd,
        Fdflags, Fdstat, Filesize, Filestat, Filetype, Fstflags, Linkcount, OptionFd, Pid, Prestat,
        Rights, Snapshot0Clockid, Sockoption, Sockstatus, Socktype, StdioMode as WasiStdioMode,
        Streamsecurity, Subscription, SubscriptionEnum, SubscriptionFsReadwrite, Tid, Timestamp,
        Tty, Whence,
    },
    *,
};
use crate::state::{bus_error_into_wasi_err, wasi_error_into_bus_err, InodeHttpSocketType};
use crate::utils::map_io_err;
use crate::WasiBusProcessId;
use crate::{
    mem_error_to_wasi,
    state::{
        self, fs_error_into_wasi_err, iterate_poll_events, net_error_into_wasi_err, poll,
        virtual_file_type_to_wasi_file_type, Inode, InodeSocket, InodeSocketKind, InodeVal, Kind,
        PollEvent, PollEventBuilder, WasiPipe, WasiState, MAX_SYMLINKS,
    },
    Fd, WasiEnv, WasiError, WasiThread, WasiThreadId,
};
use bytes::Bytes;
use std::borrow::{Borrow, Cow};
use std::convert::{Infallible, TryInto};
use std::io::{self, Read, Seek, Write};
use std::mem::transmute;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicU64;
use std::sync::{atomic::Ordering, Mutex};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tracing::{debug, error, trace, warn};
use wasmer::{
    AsStoreMut, Extern, FunctionEnv, FunctionEnvMut, Instance, Memory, Memory32, Memory64,
    MemorySize, MemoryView, Module, RuntimeError, Value, WasmPtr, WasmSlice,
};
use wasmer_vbus::{FileDescriptor, StdioMode};
use wasmer_vfs::{FsError, VirtualFile};
use wasmer_vnet::{SocketHttpRequest, StreamSecurity};

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

fn to_offset<M: MemorySize>(offset: usize) -> Result<M::Offset, Errno> {
    let ret: M::Offset = offset.try_into().map_err(|_| Errno::Inval)?;
    Ok(ret)
}

fn from_offset<M: MemorySize>(offset: M::Offset) -> Result<usize, Errno> {
    let ret: usize = offset.try_into().map_err(|_| Errno::Inval)?;
    Ok(ret)
}

fn write_bytes_inner<T: Write, M: MemorySize>(
    mut write_loc: T,
    memory: &MemoryView,
    iovs_arr_cell: WasmSlice<__wasi_ciovec_t<M>>,
) -> Result<usize, Errno> {
    let mut bytes_written = 0usize;
    for iov in iovs_arr_cell.iter() {
        let iov_inner = iov.read().map_err(mem_error_to_wasi)?;
        let bytes = WasmPtr::<u8, M>::new(iov_inner.buf)
            .slice(memory, iov_inner.buf_len)
            .map_err(mem_error_to_wasi)?;
        let bytes = bytes.read_to_vec().map_err(mem_error_to_wasi)?;
        write_loc.write_all(&bytes).map_err(map_io_err)?;

        bytes_written += from_offset::<M>(iov_inner.buf_len)?;
    }
    Ok(bytes_written)
}

pub(crate) fn write_bytes<T: Write, M: MemorySize>(
    mut write_loc: T,
    memory: &MemoryView,
    iovs_arr: WasmSlice<__wasi_ciovec_t<M>>,
) -> Result<usize, Errno> {
    let result = write_bytes_inner::<_, M>(&mut write_loc, memory, iovs_arr);
    write_loc.flush();
    result
}

pub(crate) fn read_bytes<T: Read, M: MemorySize>(
    mut reader: T,
    memory: &MemoryView,
    iovs_arr: WasmSlice<__wasi_iovec_t<M>>,
) -> Result<usize, Errno> {
    let mut bytes_read = 0usize;

    // We allocate the raw_bytes first once instead of
    // N times in the loop.
    let mut raw_bytes: Vec<u8> = vec![0; 1024];

    for iov in iovs_arr.iter() {
        let iov_inner = iov.read().map_err(mem_error_to_wasi)?;
        raw_bytes.clear();
        let to_read = from_offset::<M>(iov_inner.buf_len)?;
        raw_bytes.resize(to_read, 0);
        let has_read = reader.read(&mut raw_bytes).map_err(map_io_err)?;

        let buf = WasmPtr::<u8, M>::new(iov_inner.buf)
            .slice(memory, iov_inner.buf_len)
            .map_err(mem_error_to_wasi)?;
        buf.write_slice(&raw_bytes).map_err(mem_error_to_wasi)?;
        bytes_read += has_read;
        if has_read != to_read {
            return Ok(bytes_read);
        }
    }
    Ok(bytes_read)
}

fn __sock_actor<T, F>(
    ctx: &FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<T, Errno>
where
    F: FnOnce(&crate::state::InodeSocket) -> Result<T, Errno>,
{
    let env = ctx.data();
    let (_, state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let fd_entry = state.fs.get_fd(sock)?;
    let ret = {
        if !rights.is_empty() && !fd_entry.rights.contains(rights) {
            return Err(Errno::Access);
        }

        let inode_idx = fd_entry.inode;
        let inode = &inodes.arena[inode_idx];

        let mut guard = inode.read();
        let deref = guard.deref();
        match deref {
            Kind::Socket { socket } => actor(socket)?,
            _ => {
                return Err(Errno::Notsock);
            }
        }
    };

    Ok(ret)
}

fn __sock_actor_mut<T, F>(
    ctx: &FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<T, Errno>
where
    F: FnOnce(&mut crate::state::InodeSocket) -> Result<T, Errno>,
{
    let env = ctx.data();
    let (_, state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let fd_entry = state.fs.get_fd(sock)?;
    let ret = {
        if !rights.is_empty() && !fd_entry.rights.contains(rights) {
            return Err(Errno::Access);
        }

        let inode_idx = fd_entry.inode;
        let inode = &inodes.arena[inode_idx];

        let mut guard = inode.write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::Socket { socket } => actor(socket)?,
            _ => {
                return Err(Errno::Notsock);
            }
        }
    };

    Ok(ret)
}

fn __sock_upgrade<F>(
    ctx: &FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<(), Errno>
where
    F: FnOnce(&mut crate::state::InodeSocket) -> Result<Option<crate::state::InodeSocket>, Errno>,
{
    let env = ctx.data();
    let (_, state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let fd_entry = state.fs.get_fd(sock)?;
    if !rights.is_empty() && !fd_entry.rights.contains(rights) {
        return Err(Errno::Access);
    }

    let inode_idx = fd_entry.inode;
    let inode = &inodes.arena[inode_idx];

    let mut guard = inode.write();
    let deref_mut = guard.deref_mut();
    match deref_mut {
        Kind::Socket { socket } => {
            let new_socket = actor(socket)?;

            if let Some(mut new_socket) = new_socket {
                std::mem::swap(socket, &mut new_socket);
            }
        }
        _ => {
            return Err(Errno::Notsock);
        }
    }

    Ok(())
}

#[must_use]
fn write_buffer_array<M: MemorySize>(
    memory: &MemoryView,
    from: &[Vec<u8>],
    ptr_buffer: WasmPtr<WasmPtr<u8, M>, M>,
    buffer: WasmPtr<u8, M>,
) -> Errno {
    let ptrs = wasi_try_mem!(ptr_buffer.slice(memory, wasi_try!(to_offset::<M>(from.len()))));

    let mut current_buffer_offset = 0usize;
    for ((i, sub_buffer), ptr) in from.iter().enumerate().zip(ptrs.iter()) {
        trace!("ptr: {:?}, subbuffer: {:?}", ptr, sub_buffer);
        let mut buf_offset = buffer.offset();
        buf_offset += wasi_try!(to_offset::<M>(current_buffer_offset));
        let new_ptr = WasmPtr::new(buf_offset);
        wasi_try_mem!(ptr.write(new_ptr));

        let data =
            wasi_try_mem!(new_ptr.slice(memory, wasi_try!(to_offset::<M>(sub_buffer.len()))));
        wasi_try_mem!(data.write_slice(sub_buffer));
        wasi_try_mem!(wasi_try_mem!(
            new_ptr.add_offset(wasi_try!(to_offset::<M>(sub_buffer.len())))
        )
        .write(memory, 0));

        current_buffer_offset += sub_buffer.len() + 1;
    }

    Errno::Success
}

fn get_current_time_in_nanos() -> Result<Timestamp, Errno> {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_err(|_| Errno::Io)?;
    Ok(duration.as_nanos() as Timestamp)
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
pub fn args_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    argv: WasmPtr<WasmPtr<u8, M>, M>,
    argv_buf: WasmPtr<u8, M>,
) -> Errno {
    debug!("wasi::args_get");
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let result = write_buffer_array(&memory, &state.args, argv, argv_buf);

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
pub fn args_sizes_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    argc: WasmPtr<M::Offset, M>,
    argv_buf_size: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi::args_sizes_get");
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let argc = argc.deref(&memory);
    let argv_buf_size = argv_buf_size.deref(&memory);

    let argc_val: M::Offset = wasi_try!(state.args.len().try_into().map_err(|_| Errno::Overflow));
    let argv_buf_size_val: usize = state.args.iter().map(|v| v.len() + 1).sum();
    let argv_buf_size_val: M::Offset =
        wasi_try!(argv_buf_size_val.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(argc.write(argc_val));
    wasi_try_mem!(argv_buf_size.write(argv_buf_size_val));

    debug!("=> argc={}, argv_buf_size={}", argc_val, argv_buf_size_val);

    Errno::Success
}

/// ### `clock_res_get()`
/// Get the resolution of the specified clock
/// Input:
/// - `Clockid clock_id`
///     The ID of the clock to get the resolution of
/// Output:
/// - `Timestamp *resolution`
///     The resolution of the clock in nanoseconds
pub fn clock_res_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    resolution: WasmPtr<Timestamp, M>,
) -> Errno {
    trace!("wasi::clock_res_get");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let out_addr = resolution.deref(&memory);
    let t_out = wasi_try!(platform_clock_res_get(clock_id, out_addr));
    wasi_try_mem!(resolution.write(&memory, t_out as Timestamp));
    Errno::Success
}

/// ### `clock_time_get()`
/// Get the time of the specified clock
/// Inputs:
/// - `Clockid clock_id`
///     The ID of the clock to query
/// - `Timestamp precision`
///     The maximum amount of error the reading may have
/// Output:
/// - `Timestamp *time`
///     The value of the clock in nanoseconds
pub fn clock_time_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Snapshot0Clockid,
    precision: Timestamp,
    time: WasmPtr<Timestamp, M>,
) -> Errno {
    debug!(
        "wasi::clock_time_get clock_id: {}, precision: {}",
        clock_id as u8, precision
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let t_out = wasi_try!(platform_clock_time_get(clock_id, precision));
    wasi_try_mem!(time.write(&memory, t_out as Timestamp));

    let result = Errno::Success;
    trace!(
        "time: {} => {}",
        wasi_try_mem!(time.deref(&memory).read()),
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
pub fn environ_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    environ: WasmPtr<WasmPtr<u8, M>, M>,
    environ_buf: WasmPtr<u8, M>,
) -> Errno {
    debug!(
        "wasi::environ_get. Environ: {:?}, environ_buf: {:?}",
        environ, environ_buf
    );
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    trace!(" -> State envs: {:?}", state.envs);

    write_buffer_array(&memory, &state.envs, environ, environ_buf)
}

/// ### `environ_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *environ_count`
///     The number of environment variables.
/// - `size_t *environ_buf_size`
///     The size of the environment variable string data.
pub fn environ_sizes_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    environ_count: WasmPtr<M::Offset, M>,
    environ_buf_size: WasmPtr<M::Offset, M>,
) -> Errno {
    trace!("wasi::environ_sizes_get");
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let environ_count = environ_count.deref(&memory);
    let environ_buf_size = environ_buf_size.deref(&memory);

    let env_var_count: M::Offset =
        wasi_try!(state.envs.len().try_into().map_err(|_| Errno::Overflow));
    let env_buf_size: usize = state.envs.iter().map(|v| v.len() + 1).sum();
    let env_buf_size: M::Offset = wasi_try!(env_buf_size.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(environ_count.write(env_var_count));
    wasi_try_mem!(environ_buf_size.write(env_buf_size));

    trace!(
        "env_var_count: {}, env_buf_size: {}",
        env_var_count,
        env_buf_size
    );

    Errno::Success
}

/// ### `fd_advise()`
/// Advise the system about how a file will be used
/// Inputs:
/// - `Fd fd`
///     The file descriptor the advice applies to
/// - `Filesize offset`
///     The offset from which the advice applies
/// - `Filesize len`
///     The length from the offset to which the advice applies
/// - `__wasi_advice_t advice`
///     The advice to give
pub fn fd_advise(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
    advice: Advice,
) -> Errno {
    debug!("wasi::fd_advise: fd={}", fd);

    // this is used for our own benefit, so just returning success is a valid
    // implementation for now
    Errno::Success
}

/// ### `fd_allocate`
/// Allocate extra space for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to allocate for
/// - `Filesize offset`
///     The offset from the start marking the beginning of the allocation
/// - `Filesize len`
///     The length from the offset marking the end of the allocation
pub fn fd_allocate(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
) -> Errno {
    debug!("wasi::fd_allocate");
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;

    if !fd_entry.rights.contains(Rights::FD_ALLOCATE) {
        return Errno::Access;
    }
    let new_size = wasi_try!(offset.checked_add(len).ok_or(Errno::Inval));
    {
        let mut guard = inodes.arena[inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    wasi_try!(handle.set_len(new_size).map_err(fs_error_into_wasi_err));
                } else {
                    return Errno::Badf;
                }
            }
            Kind::Socket { .. } => return Errno::Badf,
            Kind::Pipe { .. } => return Errno::Badf,
            Kind::Buffer { buffer } => {
                buffer.resize(new_size as usize, 0);
            }
            Kind::Symlink { .. } => return Errno::Badf,
            Kind::EventNotifications { .. } => return Errno::Badf,
            Kind::Dir { .. } | Kind::Root { .. } => return Errno::Isdir,
        }
    }
    inodes.arena[inode].stat.write().unwrap().st_size = new_size;
    debug!("New file size: {}", new_size);

    Errno::Success
}

/// ### `fd_close()`
/// Close an open file descriptor
/// Inputs:
/// - `Fd fd`
///     A file descriptor mapping to an open file to close
/// Errors:
/// - `Errno::Isdir`
///     If `fd` is a directory
/// - `Errno::Badf`
///     If `fd` is invalid or not open
pub fn fd_close(ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Errno {
    debug!("wasi::fd_close: fd={}", fd);
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    wasi_try!(state.fs.close_fd(inodes.deref(), fd));

    Errno::Success
}

/// ### `fd_datasync()`
/// Synchronize the file data to disk
/// Inputs:
/// - `Fd fd`
///     The file descriptor to sync
pub fn fd_datasync(ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Errno {
    debug!("wasi::fd_datasync");
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_DATASYNC) {
        return Errno::Access;
    }

    if let Err(e) = state.fs.flush(inodes.deref(), fd) {
        e
    } else {
        Errno::Success
    }
}

/// ### `fd_fdstat_get()`
/// Get metadata of a file descriptor
/// Input:
/// - `Fd fd`
///     The file descriptor whose metadata will be accessed
/// Output:
/// - `Fdstat *buf`
///     The location where the metadata will be written
pub fn fd_fdstat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf_ptr: WasmPtr<Fdstat, M>,
) -> Errno {
    debug!(
        "wasi::fd_fdstat_get: fd={}, buf_ptr={}",
        fd,
        buf_ptr.offset()
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let stat = wasi_try!(state.fs.fdstat(inodes.deref(), fd));

    let buf = buf_ptr.deref(&memory);

    wasi_try_mem!(buf.write(stat));

    Errno::Success
}

/// ### `fd_fdstat_set_flags()`
/// Set file descriptor flags for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new flags to
/// - `Fdflags flags`
///     The flags to apply to `fd`
pub fn fd_fdstat_set_flags(ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd, flags: Fdflags) -> Errno {
    debug!("wasi::fd_fdstat_set_flags");
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(Errno::Badf));

    if !fd_entry.rights.contains(Rights::FD_FDSTAT_SET_FLAGS) {
        return Errno::Access;
    }

    fd_entry.flags = flags;
    Errno::Success
}

/// ### `fd_fdstat_set_rights()`
/// Set the rights of a file descriptor.  This can only be used to remove rights
/// Inputs:
/// - `Fd fd`
///     The file descriptor to apply the new rights to
/// - `Rights fs_rights_base`
///     The rights to apply to `fd`
/// - `Rights fs_rights_inheriting`
///     The inheriting rights to apply to `fd`
pub fn fd_fdstat_set_rights(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
) -> Errno {
    debug!("wasi::fd_fdstat_set_rights");
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(Errno::Badf));

    // ensure new rights are a subset of current rights
    if fd_entry.rights | fs_rights_base != fd_entry.rights
        || fd_entry.rights_inheriting | fs_rights_inheriting != fd_entry.rights_inheriting
    {
        return Errno::Notcapable;
    }

    fd_entry.rights = fs_rights_base;
    fd_entry.rights_inheriting = fs_rights_inheriting;

    Errno::Success
}

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `Fd fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `Filestat *buf`
///     Where the metadata from `fd` will be written
pub fn fd_filestat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    debug!("wasi::fd_filestat_get");
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_FILESTAT_GET) {
        return Errno::Access;
    }

    let stat = wasi_try!(state.fs.filestat_fd(inodes.deref(), fd));

    let buf = buf.deref(&memory);
    wasi_try_mem!(buf.write(stat));

    Errno::Success
}

/// ### `fd_filestat_set_size()`
/// Change the size of an open file, zeroing out any new bytes
/// Inputs:
/// - `Fd fd`
///     File descriptor to adjust
/// - `Filesize st_size`
///     New size that `fd` will be set to
pub fn fd_filestat_set_size(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_size: Filesize,
) -> Errno {
    debug!("wasi::fd_filestat_set_size");
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;

    if !fd_entry.rights.contains(Rights::FD_FILESTAT_SET_SIZE) {
        return Errno::Access;
    }

    {
        let mut guard = inodes.arena[inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    wasi_try!(handle.set_len(st_size).map_err(fs_error_into_wasi_err));
                } else {
                    return Errno::Badf;
                }
            }
            Kind::Buffer { buffer } => {
                buffer.resize(st_size as usize, 0);
            }
            Kind::Socket { .. } => return Errno::Badf,
            Kind::Pipe { .. } => return Errno::Badf,
            Kind::Symlink { .. } => return Errno::Badf,
            Kind::EventNotifications { .. } => return Errno::Badf,
            Kind::Dir { .. } | Kind::Root { .. } => return Errno::Isdir,
        }
    }
    inodes.arena[inode].stat.write().unwrap().st_size = st_size;

    Errno::Success
}

/// ### `fd_filestat_set_times()`
/// Set timestamp metadata on a file
/// Inputs:
/// - `Timestamp st_atim`
///     Last accessed time
/// - `Timestamp st_mtim`
///     Last modified time
/// - `Fstflags fst_flags`
///     Bit-vector for controlling which times get set
pub fn fd_filestat_set_times(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Errno {
    debug!("wasi::fd_filestat_set_times");
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_FILESTAT_SET_TIMES) {
        return Errno::Access;
    }

    if (fst_flags.contains(Fstflags::SET_ATIM) && fst_flags.contains(Fstflags::SET_ATIM_NOW))
        || (fst_flags.contains(Fstflags::SET_MTIM) && fst_flags.contains(Fstflags::SET_MTIM_NOW))
    {
        return Errno::Inval;
    }

    let inode_idx = fd_entry.inode;
    let inode = &inodes.arena[inode_idx];

    if fst_flags.contains(Fstflags::SET_ATIM) || fst_flags.contains(Fstflags::SET_ATIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_ATIM) {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_atim = time_to_set;
    }

    if fst_flags.contains(Fstflags::SET_MTIM) || fst_flags.contains(Fstflags::SET_MTIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_MTIM) {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_mtim = time_to_set;
    }

    Errno::Success
}

/// ### `fd_pread()`
/// Read from the file at the given offset without updating the file cursor.
/// This acts like a stateless version of Seek + Read
/// Inputs:
/// - `Fd fd`
///     The file descriptor to read the data with
/// - `const __wasi_iovec_t* iovs'
///     Vectors where the data will be stored
/// - `size_t iovs_len`
///     The number of vectors to store the data into
/// - `Filesize offset`
///     The file cursor to use: the starting position from which data will be read
/// Output:
/// - `size_t nread`
///     The number of bytes read
pub fn fd_pread<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!("wasi::fd_pread: fd={}, offset={}", fd, offset);
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let iovs = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
    let nread_ref = nread.deref(&memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_read = match fd {
        __WASI_STDIN_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdin_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                env
            );
            if let Some(ref mut stdin) = guard.deref_mut() {
                wasi_try_ok!(read_bytes(stdin, &memory, iovs), env)
            } else {
                return Ok(Errno::Badf);
            }
        }
        __WASI_STDOUT_FILENO => return Ok(Errno::Inval),
        __WASI_STDERR_FILENO => return Ok(Errno::Inval),
        _ => {
            let inode = fd_entry.inode;

            if !fd_entry.rights.contains(Rights::FD_READ | Rights::FD_SEEK) {
                debug!(
                    "Invalid rights on {:X}: expected READ and SEEK",
                    fd_entry.rights
                );
                return Ok(Errno::Access);
            }
            let mut guard = inodes.arena[inode].write();
            let deref_mut = guard.deref_mut();
            match deref_mut {
                Kind::File { handle, .. } => {
                    if let Some(h) = handle {
                        wasi_try_ok!(
                            h.seek(std::io::SeekFrom::Start(offset as u64))
                                .map_err(map_io_err),
                            env
                        );
                        wasi_try_ok!(read_bytes(h, &memory, iovs), env)
                    } else {
                        return Ok(Errno::Inval);
                    }
                }
                Kind::Socket { socket } => {
                    wasi_try_ok!(socket.recv(&memory, iovs), env)
                }
                Kind::Pipe { pipe } => {
                    wasi_try_ok!(pipe.recv(&memory, iovs), env)
                }
                Kind::EventNotifications { .. } => return Ok(Errno::Inval),
                Kind::Dir { .. } | Kind::Root { .. } => return Ok(Errno::Isdir),
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_pread"),
                Kind::Buffer { buffer } => {
                    wasi_try_ok!(read_bytes(&buffer[(offset as usize)..], &memory, iovs), env)
                }
            }
        }
    };

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(nread_ref.write(bytes_read));
    debug!("Success: {} bytes read", bytes_read);
    Ok(Errno::Success)
}

/// ### `fd_prestat_get()`
/// Get metadata about a preopened file descriptor
/// Input:
/// - `Fd fd`
///     The preopened file descriptor to query
/// Output:
/// - `__wasi_prestat *buf`
///     Where the metadata will be written
pub fn fd_prestat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Prestat, M>,
) -> Errno {
    trace!("wasi::fd_prestat_get: fd={}", fd);
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let prestat_ptr = buf.deref(&memory);
    wasi_try_mem!(
        prestat_ptr.write(wasi_try!(state.fs.prestat_fd(inodes.deref(), fd).map_err(
            |code| {
                debug!("fd_prestat_get failed (fd={}) - errno={}", fd, code);
                code
            }
        )))
    );

    Errno::Success
}

pub fn fd_prestat_dir_name<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    trace!(
        "wasi::fd_prestat_dir_name: fd={}, path_len={}",
        fd,
        path_len
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let path_chars = wasi_try_mem!(path.slice(&memory, path_len));

    let real_inode = wasi_try!(state.fs.get_fd_inode(fd));
    let inode_val = &inodes.arena[real_inode];

    // check inode-val.is_preopened?

    trace!("=> inode: {:?}", inode_val);
    let guard = inode_val.read();
    let deref = guard.deref();
    match deref {
        Kind::Dir { .. } | Kind::Root { .. } => {
            let path_len: u64 = path_len.into();
            if (inode_val.name.len() as u64) <= path_len {
                wasi_try_mem!(path_chars
                    .subslice(0..inode_val.name.len() as u64)
                    .write_slice(inode_val.name.as_bytes()));

                trace!("=> result: \"{}\"", inode_val.name);

                Errno::Success
            } else {
                Errno::Overflow
            }
        }
        Kind::Symlink { .. }
        | Kind::Buffer { .. }
        | Kind::File { .. }
        | Kind::Socket { .. }
        | Kind::Pipe { .. }
        | Kind::EventNotifications { .. } => Errno::Notdir,
    }
}

/// ### `fd_pwrite()`
/// Write to a file without adjusting its offset
/// Inputs:
/// - `Fd`
///     File descriptor (opened with writing) to write to
/// - `const __wasi_ciovec_t *iovs`
///     List of vectors to read data from
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// - `Filesize offset`
///     The offset to write at
/// Output:
/// - `u32 *nwritten`
///     Number of bytes written
pub fn fd_pwrite<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!("wasi::fd_pwrite");
    // TODO: refactor, this is just copied from `fd_write`...
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
    let nwritten_ref = nwritten.deref(&memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_written = match fd {
        __WASI_STDIN_FILENO => return Ok(Errno::Inval),
        __WASI_STDOUT_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdout_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                env
            );
            if let Some(ref mut stdout) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stdout, &memory, iovs_arr), env)
            } else {
                return Ok(Errno::Badf);
            }
        }
        __WASI_STDERR_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stderr_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                env
            );
            if let Some(ref mut stderr) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stderr, &memory, iovs_arr), env)
            } else {
                return Ok(Errno::Badf);
            }
        }
        _ => {
            if !fd_entry.rights.contains(Rights::FD_WRITE | Rights::FD_SEEK) {
                return Ok(Errno::Access);
            }

            let inode_idx = fd_entry.inode;
            let inode = &inodes.arena[inode_idx];

            let mut guard = inode.write();
            let deref_mut = guard.deref_mut();
            match deref_mut {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        wasi_try_ok!(
                            handle
                                .seek(std::io::SeekFrom::Start(offset as u64))
                                .map_err(map_io_err),
                            env
                        );
                        wasi_try_ok!(write_bytes(handle, &memory, iovs_arr), env)
                    } else {
                        return Ok(Errno::Inval);
                    }
                }
                Kind::Socket { socket } => {
                    wasi_try_ok!(socket.send(&memory, iovs_arr), env)
                }
                Kind::Pipe { pipe } => {
                    wasi_try_ok!(pipe.send(&memory, iovs_arr), env)
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(Errno::Isdir);
                }
                Kind::EventNotifications { .. } => return Ok(Errno::Inval),
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_pwrite"),
                Kind::Buffer { buffer } => {
                    wasi_try_ok!(
                        write_bytes(&mut buffer[(offset as usize)..], &memory, iovs_arr),
                        env
                    )
                }
            }
        }
    };

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(nwritten_ref.write(bytes_written));

    Ok(Errno::Success)
}

/// ### `fd_read()`
/// Read data from file descriptor
/// Inputs:
/// - `Fd fd`
///     File descriptor from which data will be read
/// - `const __wasi_iovec_t *iovs`
///     Vectors where data will be stored
/// - `u32 iovs_len`
///     Length of data in `iovs`
/// Output:
/// - `u32 *nread`
///     Number of bytes read
///
pub fn fd_read<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!("wasi::fd_read: fd={}", fd);
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    //let iovs_len = if iovs_len > M::Offset::from(1u32) { M::Offset::from(1u32) } else { iovs_len };
    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
    let nread_ref = nread.deref(&memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_read = match fd {
        __WASI_STDIN_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdin_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                env
            );
            if let Some(ref mut stdin) = guard.deref_mut() {
                wasi_try_ok!(read_bytes(stdin, &memory, iovs_arr), env)
            } else {
                return Ok(Errno::Badf);
            }
        }
        __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => return Ok(Errno::Inval),
        _ => {
            if !fd_entry.rights.contains(Rights::FD_READ) {
                // TODO: figure out the error to return when lacking rights
                return Ok(Errno::Access);
            }

            let is_non_blocking = fd_entry.flags.contains(Fdflags::NONBLOCK);
            let offset = fd_entry.offset as usize;
            let inode_idx = fd_entry.inode;
            let inode = &inodes.arena[inode_idx];

            let bytes_read = {
                let mut guard = inode.write();
                let deref_mut = guard.deref_mut();
                match deref_mut {
                    Kind::File { handle, .. } => {
                        if let Some(handle) = handle {
                            wasi_try_ok!(
                                handle
                                    .seek(std::io::SeekFrom::Start(offset as u64))
                                    .map_err(map_io_err),
                                env
                            );
                            wasi_try_ok!(read_bytes(handle, &memory, iovs_arr), env)
                        } else {
                            return Ok(Errno::Inval);
                        }
                    }
                    Kind::Socket { socket } => {
                        wasi_try_ok!(socket.recv(&memory, iovs_arr), env)
                    }
                    Kind::Pipe { pipe } => {
                        wasi_try_ok!(pipe.recv(&memory, iovs_arr), env)
                    }
                    Kind::Dir { .. } | Kind::Root { .. } => {
                        // TODO: verify
                        return Ok(Errno::Isdir);
                    }
                    Kind::EventNotifications {
                        counter,
                        is_semaphore,
                        wakers,
                    } => {
                        let counter = Arc::clone(counter);
                        let is_semaphore: bool = *is_semaphore;
                        let wakers = Arc::clone(wakers);
                        drop(guard);
                        drop(inodes);

                        let (tx, rx) = mpsc::channel();
                        {
                            let mut guard = wakers.lock().unwrap();
                            guard.push_front(tx);
                        }

                        let ret;
                        loop {
                            let val = counter.load(Ordering::Acquire);
                            if val > 0 {
                                let new_val = if is_semaphore { val - 1 } else { 0 };
                                if counter
                                    .compare_exchange(
                                        val,
                                        new_val,
                                        Ordering::AcqRel,
                                        Ordering::Acquire,
                                    )
                                    .is_ok()
                                {
                                    let reader = val.to_ne_bytes();
                                    ret = wasi_try_ok!(
                                        read_bytes(&reader[..], &memory, iovs_arr),
                                        env
                                    );
                                    break;
                                } else {
                                    continue;
                                }
                            }

                            // If its none blocking then exit
                            if is_non_blocking {
                                return Ok(Errno::Again);
                            }

                            // Yield for a fixed period of time and then check again
                            env.yield_now()?;
                            if rx.recv_timeout(Duration::from_millis(5)).is_err() {
                                env.sleep(Duration::from_millis(5))?;
                            }
                        }
                        ret
                    }
                    Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_read"),
                    Kind::Buffer { buffer } => {
                        wasi_try_ok!(read_bytes(&buffer[offset..], &memory, iovs_arr), env)
                    }
                }
            };

            // reborrow
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            fd_entry.offset += bytes_read as u64;

            bytes_read
        }
    };
    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(nread_ref.write(bytes_read));

    Ok(Errno::Success)
}

/// ### `fd_readdir()`
/// Read data from directory specified by file descriptor
/// Inputs:
/// - `Fd fd`
///     File descriptor from which directory data will be read
/// - `void *buf`
///     Buffer where directory entries are stored
/// - `u32 buf_len`
///     Length of data in `buf`
/// - `Dircookie cookie`
///     Where the directory reading should start from
/// Output:
/// - `u32 *bufused`
///     The Number of bytes stored in `buf`; if less than `buf_len` then entire
///     directory has been read
pub fn fd_readdir<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    cookie: Dircookie,
    bufused: WasmPtr<M::Offset, M>,
) -> Errno {
    trace!("wasi::fd_readdir");
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    // TODO: figure out how this is supposed to work;
    // is it supposed to pack the buffer full every time until it can't? or do one at a time?

    let buf_arr = wasi_try_mem!(buf.slice(&memory, buf_len));
    let bufused_ref = bufused.deref(&memory);
    let working_dir = wasi_try!(state.fs.get_fd(fd));
    let mut cur_cookie = cookie;
    let mut buf_idx = 0usize;

    let entries: Vec<(String, Filetype, u64)> = {
        let guard = inodes.arena[working_dir.inode].read();
        let deref = guard.deref();
        match deref {
            Kind::Dir { path, entries, .. } => {
                debug!("Reading dir {:?}", path);
                // TODO: refactor this code
                // we need to support multiple calls,
                // simple and obviously correct implementation for now:
                // maintain consistent order via lexacographic sorting
                let fs_info = wasi_try!(wasi_try!(state.fs_read_dir(path))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(fs_error_into_wasi_err));
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
                    .collect::<Result<Vec<(String, Filetype, u64)>, _>>());
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
                // adding . and .. special folders
                // TODO: inode
                entry_vec.push((".".to_string(), Filetype::Directory, 0));
                entry_vec.push(("..".to_string(), Filetype::Directory, 0));
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
            Kind::File { .. }
            | Kind::Symlink { .. }
            | Kind::Buffer { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. } => return Errno::Notdir,
        }
    };

    for (entry_path_str, wasi_file_type, ino) in entries.iter().skip(cookie as usize) {
        cur_cookie += 1;
        let namlen = entry_path_str.len();
        debug!("Returning dirent for {}", entry_path_str);
        let dirent = Dirent {
            d_next: cur_cookie,
            d_ino: *ino,
            d_namlen: namlen as u32,
            d_type: *wasi_file_type,
        };
        let dirent_bytes = dirent_to_le_bytes(&dirent);
        let buf_len: u64 = buf_len.into();
        let upper_limit = std::cmp::min(
            (buf_len - buf_idx as u64) as usize,
            std::mem::size_of::<Dirent>(),
        );
        for (i, b) in dirent_bytes.iter().enumerate().take(upper_limit) {
            wasi_try_mem!(buf_arr.index((i + buf_idx) as u64).write(*b));
        }
        buf_idx += upper_limit;
        if upper_limit != std::mem::size_of::<Dirent>() {
            break;
        }
        let upper_limit = std::cmp::min((buf_len - buf_idx as u64) as usize, namlen);
        for (i, b) in entry_path_str.bytes().take(upper_limit).enumerate() {
            wasi_try_mem!(buf_arr.index((i + buf_idx) as u64).write(b));
        }
        buf_idx += upper_limit;
        if upper_limit != namlen {
            break;
        }
    }

    let buf_idx: M::Offset = wasi_try!(buf_idx.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(bufused_ref.write(buf_idx));
    Errno::Success
}

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `Fd from`
///     File descriptor to copy
/// - `Fd to`
///     Location to copy file descriptor to
pub fn fd_renumber(ctx: FunctionEnvMut<'_, WasiEnv>, from: WasiFd, to: WasiFd) -> Errno {
    debug!("wasi::fd_renumber: from={}, to={}", from, to);
    let env = ctx.data();
    let (_, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&from).ok_or(Errno::Badf));

    let new_fd_entry = Fd {
        // TODO: verify this is correct
        rights: fd_entry.rights_inheriting,
        ..*fd_entry
    };

    fd_map.insert(to, new_fd_entry);
    fd_map.remove(&from);
    Errno::Success
}

/// ### `fd_dup()`
/// Duplicates the file handle
/// Inputs:
/// - `Fd fd`
///   File handle to be cloned
/// Outputs:
/// - `Fd fd`
///   The new file handle that is a duplicate of the original
pub fn fd_dup<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    ret_fd: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!("wasi::fd_dup");

    let env = ctx.data();
    let (memory, state) = env.get_memory_and_wasi_state(&ctx, 0);
    let fd = wasi_try!(state.fs.clone_fd(fd));

    wasi_try_mem!(ret_fd.write(&memory, fd));

    Errno::Success
}

/// ### `fd_event()`
/// Creates a file handle for event notifications
pub fn fd_event<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    initial_val: u64,
    flags: EventFdFlags,
    ret_fd: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!("wasi::fd_event");

    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::EventNotifications {
        counter: Arc::new(AtomicU64::new(initial_val)),
        is_semaphore: flags & EVENT_FD_FLAGS_SEMAPHORE != 0,
        wakers: Default::default(),
    };

    let inode = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind,
        false,
        "event".to_string(),
    );
    let rights = Rights::FD_READ | Rights::FD_WRITE | Rights::POLL_FD_READWRITE;
    let fd = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    wasi_try_mem!(ret_fd.write(&memory, fd));

    Errno::Success
}

/// ### `fd_seek()`
/// Update file descriptor offset
/// Inputs:
/// - `Fd fd`
///     File descriptor to mutate
/// - `FileDelta offset`
///     Number of bytes to adjust offset by
/// - `Whence whence`
///     What the offset is relative to
/// Output:
/// - `Filesize *fd`
///     The new offset relative to the start of the file
pub fn fd_seek<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: FileDelta,
    whence: Whence,
    newoffset: WasmPtr<Filesize, M>,
) -> Result<Errno, WasiError> {
    trace!("wasi::fd_seek: fd={}, offset={}", fd, offset);
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let new_offset_ref = newoffset.deref(&memory);
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_SEEK) {
        return Ok(Errno::Access);
    }

    // TODO: handle case if fd is a dir?
    match whence {
        Whence::Cur => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            fd_entry.offset = (fd_entry.offset as i64 + offset) as u64
        }
        Whence::End => {
            use std::io::SeekFrom;
            let inode_idx = fd_entry.inode;
            let mut guard = inodes.arena[inode_idx].write();
            let deref_mut = guard.deref_mut();
            match deref_mut {
                Kind::File { ref mut handle, .. } => {
                    if let Some(handle) = handle {
                        let end =
                            wasi_try_ok!(handle.seek(SeekFrom::End(0)).map_err(map_io_err), env);

                        // TODO: handle case if fd_entry.offset uses 64 bits of a u64
                        drop(guard);
                        let mut fd_map = state.fs.fd_map.write().unwrap();
                        let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
                        fd_entry.offset = (end as i64 + offset) as u64;
                    } else {
                        return Ok(Errno::Inval);
                    }
                }
                Kind::Symlink { .. } => {
                    unimplemented!("wasi::fd_seek not implemented for symlinks")
                }
                Kind::Dir { .. }
                | Kind::Root { .. }
                | Kind::Socket { .. }
                | Kind::Pipe { .. }
                | Kind::EventNotifications { .. } => {
                    // TODO: check this
                    return Ok(Errno::Inval);
                }
                Kind::Buffer { .. } => {
                    // seeking buffers probably makes sense
                    // TODO: implement this
                    return Ok(Errno::Inval);
                }
            }
        }
        Whence::Set => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            fd_entry.offset = offset as u64
        }
        _ => return Ok(Errno::Inval),
    }
    // reborrow
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    wasi_try_mem_ok!(new_offset_ref.write(fd_entry.offset));

    Ok(Errno::Success)
}

/// ### `fd_sync()`
/// Synchronize file and metadata to disk (TODO: expand upon what this means in our system)
/// Inputs:
/// - `Fd fd`
///     The file descriptor to sync
/// Errors:
/// TODO: figure out which errors this should return
/// - `Errno::Perm`
/// - `Errno::Notcapable`
pub fn fd_sync(ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Errno {
    debug!("wasi::fd_sync");
    debug!("=> fd={}", fd);
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_SYNC) {
        return Errno::Access;
    }
    let inode = fd_entry.inode;

    // TODO: implement this for more than files
    {
        let mut guard = inodes.arena[inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::File { handle, .. } => {
                if let Some(h) = handle {
                    wasi_try!(h.sync_to_disk().map_err(fs_error_into_wasi_err));
                } else {
                    return Errno::Inval;
                }
            }
            Kind::Root { .. } | Kind::Dir { .. } => return Errno::Isdir,
            Kind::Buffer { .. }
            | Kind::Symlink { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. } => return Errno::Inval,
        }
    }

    Errno::Success
}

/// ### `fd_tell()`
/// Get the offset of the file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to access
/// Output:
/// - `Filesize *offset`
///     The offset of `fd` relative to the start of the file
pub fn fd_tell<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: WasmPtr<Filesize, M>,
) -> Errno {
    debug!("wasi::fd_tell");
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let offset_ref = offset.deref(&memory);

    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_TELL) {
        return Errno::Access;
    }

    wasi_try_mem!(offset_ref.write(fd_entry.offset));

    Errno::Success
}

/// ### `fd_write()`
/// Write data to the file descriptor
/// Inputs:
/// - `Fd`
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
pub fn fd_write<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!("wasi::fd_write: fd={}", fd);
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
    let nwritten_ref = nwritten.deref(&memory);

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_written = match fd {
        __WASI_STDIN_FILENO => return Ok(Errno::Inval),
        __WASI_STDOUT_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stdout_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                env
            );
            if let Some(ref mut stdout) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stdout, &memory, iovs_arr), env)
            } else {
                return Ok(Errno::Badf);
            }
        }
        __WASI_STDERR_FILENO => {
            let mut guard = wasi_try_ok!(
                inodes
                    .stderr_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err),
                env
            );
            if let Some(ref mut stderr) = guard.deref_mut() {
                wasi_try_ok!(write_bytes(stderr, &memory, iovs_arr), env)
            } else {
                return Ok(Errno::Badf);
            }
        }
        _ => {
            if !fd_entry.rights.contains(Rights::FD_WRITE) {
                return Ok(Errno::Access);
            }

            let offset = fd_entry.offset as usize;
            let inode_idx = fd_entry.inode;
            let inode = &inodes.arena[inode_idx];

            let bytes_written = {
                let mut guard = inode.write();
                let deref_mut = guard.deref_mut();
                match deref_mut {
                    Kind::File { handle, .. } => {
                        if let Some(handle) = handle {
                            wasi_try_ok!(
                                handle
                                    .seek(std::io::SeekFrom::Start(offset as u64))
                                    .map_err(map_io_err),
                                env
                            );
                            wasi_try_ok!(write_bytes(handle, &memory, iovs_arr), env)
                        } else {
                            return Ok(Errno::Inval);
                        }
                    }
                    Kind::Socket { socket } => {
                        wasi_try_ok!(socket.send(&memory, iovs_arr), env)
                    }
                    Kind::Pipe { pipe } => {
                        wasi_try_ok!(pipe.send(&memory, iovs_arr), env)
                    }
                    Kind::Dir { .. } | Kind::Root { .. } => {
                        // TODO: verify
                        return Ok(Errno::Isdir);
                    }
                    Kind::EventNotifications {
                        counter, wakers, ..
                    } => {
                        let mut val = 0u64.to_ne_bytes();
                        let written = wasi_try_ok!(write_bytes(&mut val[..], &memory, iovs_arr));
                        if written != val.len() {
                            return Ok(Errno::Inval);
                        }
                        let val = u64::from_ne_bytes(val);

                        counter.fetch_add(val, Ordering::AcqRel);
                        {
                            let mut guard = wakers.lock().unwrap();
                            while let Some(wake) = guard.pop_back() {
                                if wake.send(()).is_ok() {
                                    break;
                                }
                            }
                        }

                        written
                    }
                    Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_write"),
                    Kind::Buffer { buffer } => {
                        wasi_try_ok!(write_bytes(&mut buffer[offset..], &memory, iovs_arr), env)
                    }
                }
            };

            // reborrow
            {
                let mut fd_map = state.fs.fd_map.write().unwrap();
                let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
                fd_entry.offset += bytes_written as u64;
            }
            wasi_try_ok!(state.fs.filestat_resync_size(inodes.deref(), fd), env);

            bytes_written
        }
    };

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(nwritten_ref.write(bytes_written));

    Ok(Errno::Success)
}

/// ### `fd_pipe()`
/// Creates ta pipe that feeds data between two file handles
/// Output:
/// - `Fd`
///     First file handle that represents one end of the pipe
/// - `Fd`
///     Second file handle that represents the other end of the pipe
pub fn fd_pipe<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ro_fd1: WasmPtr<WasiFd, M>,
    ro_fd2: WasmPtr<WasiFd, M>,
) -> Errno {
    trace!("wasi::fd_pipe");

    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let (pipe1, pipe2) = WasiPipe::new();

    let inode1 = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        Kind::Pipe { pipe: pipe1 },
        false,
        "pipe".to_string(),
    );
    let inode2 = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        Kind::Pipe { pipe: pipe2 },
        false,
        "pipe".to_string(),
    );

    let rights = Rights::all_socket();
    let fd1 = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode1));
    let fd2 = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode2));

    wasi_try_mem!(ro_fd1.write(&memory, fd1));
    wasi_try_mem!(ro_fd2.write(&memory, fd2));

    Errno::Success
}

/// ### `path_create_directory()`
/// Create directory at a path
/// Inputs:
/// - `Fd fd`
///     The directory that the path is relative to
/// - `const char *path`
///     String containing path data
/// - `u32 path_len`
///     The length of `path`
/// Errors:
/// Required Rights:
/// - Rights::PATH_CREATE_DIRECTORY
///     This right must be set on the directory that the file is created in (TODO: verify that this is true)
pub fn path_create_directory<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    debug!("wasi::path_create_directory");
    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let working_dir = wasi_try!(state.fs.get_fd(fd));
    {
        let guard = inodes.arena[working_dir.inode].read();
        if let Kind::Root { .. } = guard.deref() {
            return Errno::Access;
        }
    }
    if !working_dir.rights.contains(Rights::PATH_CREATE_DIRECTORY) {
        return Errno::Access;
    }
    let path_string = unsafe { get_input_str!(&memory, path, path_len) };
    debug!("=> fd: {}, path: {}", fd, &path_string);

    let path = std::path::PathBuf::from(&path_string);
    let path_vec = wasi_try!(path
        .components()
        .map(|comp| {
            comp.as_os_str()
                .to_str()
                .map(|inner_str| inner_str.to_string())
                .ok_or(Errno::Inval)
        })
        .collect::<Result<Vec<String>, Errno>>());
    if path_vec.is_empty() {
        return Errno::Inval;
    }

    debug!("Looking at components {:?}", &path_vec);

    let mut cur_dir_inode = working_dir.inode;
    for comp in &path_vec {
        debug!("Creating dir {}", comp);
        let mut guard = inodes.arena[cur_dir_inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
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
                        &memory,
                        state,
                        inodes.deref_mut(),
                        fd,
                        0,
                        &adjusted_path.to_string_lossy(),
                    ) {
                        if adjusted_path_stat.st_filetype != Filetype::Directory {
                            return Errno::Notdir;
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
            Kind::Root { .. } => return Errno::Access,
            _ => return Errno::Notdir,
        }
    }

    Errno::Success
}

/// ### `path_filestat_get()`
/// Access metadata about a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory that `path` is relative to
/// - `LookupFlags flags`
///     Flags to control how `path` is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// Output:
/// - `__wasi_file_stat_t *buf`
///     The location where the metadata will be stored
pub fn path_filestat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    debug!("wasi::path_filestat_get (fd={})", fd);
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let path_string = unsafe { get_input_str!(&memory, path, path_len) };

    let stat = wasi_try!(path_filestat_get_internal(
        &memory,
        state,
        inodes.deref_mut(),
        fd,
        flags,
        &path_string
    ));

    wasi_try_mem!(buf.deref(&memory).write(stat));

    Errno::Success
}

/// ### `path_filestat_get()`
/// Access metadata about a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory that `path` is relative to
/// - `LookupFlags flags`
///     Flags to control how `path` is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// Output:
/// - `__wasi_file_stat_t *buf`
///     The location where the metadata will be stored
pub fn path_filestat_get_internal(
    memory: &MemoryView,
    state: &WasiState,
    inodes: &mut crate::WasiInodes,
    fd: WasiFd,
    flags: LookupFlags,
    path_string: &str,
) -> Result<Filestat, Errno> {
    let root_dir = state.fs.get_fd(fd)?;

    if !root_dir.rights.contains(Rights::PATH_FILESTAT_GET) {
        return Err(Errno::Access);
    }
    debug!("=> base_fd: {}, path: {}", fd, path_string);

    let file_inode = state.fs.get_inode_at_path(
        inodes,
        fd,
        path_string,
        flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0,
    )?;
    if inodes.arena[file_inode].is_preopened {
        Ok(*inodes.arena[file_inode].stat.read().unwrap().deref())
    } else {
        let guard = inodes.arena[file_inode].read();
        state.fs.get_stat_for_kind(inodes.deref(), guard.deref())
    }
}

/// ### `path_filestat_set_times()`
/// Update time metadata on a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory relative to which the path is resolved
/// - `LookupFlags flags`
///     Flags to control how the path is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// - `Timestamp st_atim`
///     The timestamp that the last accessed time attribute is set to
/// -  `Timestamp st_mtim`
///     The timestamp that the last modified time attribute is set to
/// - `Fstflags fst_flags`
///     A bitmask controlling which attributes are set
pub fn path_filestat_set_times<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    st_atim: Timestamp,
    st_mtim: Timestamp,
    fst_flags: Fstflags,
) -> Errno {
    debug!("wasi::path_filestat_set_times");
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let fd_inode = fd_entry.inode;
    if !fd_entry.rights.contains(Rights::PATH_FILESTAT_SET_TIMES) {
        return Errno::Access;
    }
    if (fst_flags.contains(Fstflags::SET_ATIM) && fst_flags.contains(Fstflags::SET_ATIM_NOW))
        || (fst_flags.contains(Fstflags::SET_MTIM) && fst_flags.contains(Fstflags::SET_MTIM_NOW))
    {
        return Errno::Inval;
    }

    let path_string = unsafe { get_input_str!(&memory, path, path_len) };
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

    if fst_flags.contains(Fstflags::SET_ATIM) || fst_flags.contains(Fstflags::SET_ATIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_ATIM) {
            st_atim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_atim = time_to_set;
    }
    if fst_flags.contains(Fstflags::SET_MTIM) || fst_flags.contains(Fstflags::SET_MTIM_NOW) {
        let time_to_set = if fst_flags.contains(Fstflags::SET_MTIM) {
            st_mtim
        } else {
            wasi_try!(get_current_time_in_nanos())
        };
        inode.stat.write().unwrap().st_mtim = time_to_set;
    }

    Errno::Success
}

/// ### `path_link()`
/// Create a hard link
/// Inputs:
/// - `Fd old_fd`
///     The directory relative to which the `old_path` is
/// - `LookupFlags old_flags`
///     Flags to control how `old_path` is understood
/// - `const char *old_path`
///     String containing the old file path
/// - `u32 old_path_len`
///     Length of the `old_path` string
/// - `Fd new_fd`
///     The directory relative to which the `new_path` is
/// - `const char *new_path`
///     String containing the new file path
/// - `u32 old_path_len`
///     Length of the `new_path` string
pub fn path_link<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    old_fd: WasiFd,
    old_flags: LookupFlags,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    new_fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Errno {
    debug!("wasi::path_link");
    if old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
    let old_path_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    let new_path_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
    let source_fd = wasi_try!(state.fs.get_fd(old_fd));
    let target_fd = wasi_try!(state.fs.get_fd(new_fd));
    debug!(
        "=> source_fd: {}, source_path: {}, target_fd: {}, target_path: {}",
        old_fd, &old_path_str, new_fd, new_path_str
    );

    if !source_fd.rights.contains(Rights::PATH_LINK_SOURCE)
        || !target_fd.rights.contains(Rights::PATH_LINK_TARGET)
    {
        return Errno::Access;
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

    if inodes.arena[source_inode].stat.write().unwrap().st_nlink == Linkcount::max_value() {
        return Errno::Mlink;
    }
    {
        let mut guard = inodes.arena[target_parent_inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::Dir { entries, .. } => {
                if entries.contains_key(&new_entry_name) {
                    return Errno::Exist;
                }
                entries.insert(new_entry_name, source_inode);
            }
            Kind::Root { .. } => return Errno::Inval,
            Kind::File { .. }
            | Kind::Symlink { .. }
            | Kind::Buffer { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. } => return Errno::Notdir,
        }
    }
    inodes.arena[source_inode].stat.write().unwrap().st_nlink += 1;

    Errno::Success
}

/// ### `path_open()`
/// Open file located at the given path
/// Inputs:
/// - `Fd dirfd`
///     The fd corresponding to the directory that the file is in
/// - `LookupFlags dirflags`
///     Flags specifying how the path will be resolved
/// - `char *path`
///     The path of the file or directory to open
/// - `u32 path_len`
///     The length of the `path` string
/// - `Oflags o_flags`
///     How the file will be opened
/// - `Rights fs_rights_base`
///     The rights of the created file descriptor
/// - `Rights fs_rightsinheriting`
///     The rights of file descriptors derived from the created file descriptor
/// - `Fdflags fs_flags`
///     The flags of the file descriptor
/// Output:
/// - `Fd* fd`
///     The new file descriptor
/// Possible Errors:
/// - `Errno::Access`, `Errno::Badf`, `Errno::Fault`, `Errno::Fbig?`, `Errno::Inval`, `Errno::Io`, `Errno::Loop`, `Errno::Mfile`, `Errno::Nametoolong?`, `Errno::Nfile`, `Errno::Noent`, `Errno::Notdir`, `Errno::Rofs`, and `Errno::Notcapable`
pub fn path_open<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    dirfd: WasiFd,
    dirflags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    o_flags: Oflags,
    fs_rights_base: Rights,
    fs_rights_inheriting: Rights,
    fs_flags: Fdflags,
    fd: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!("wasi::path_open");
    if dirflags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
    /* TODO: find actual upper bound on name size (also this is a path, not a name :think-fish:) */
    let path_len64: u64 = path_len.into();
    if path_len64 > 1024u64 * 1024u64 {
        return Errno::Nametoolong;
    }

    let fd_ref = fd.deref(&memory);

    // o_flags:
    // - __WASI_O_CREAT (create if it does not exist)
    // - __WASI_O_DIRECTORY (fail if not dir)
    // - __WASI_O_EXCL (fail if file exists)
    // - __WASI_O_TRUNC (truncate size to 0)

    let working_dir = wasi_try!(state.fs.get_fd(dirfd));
    let working_dir_rights_inheriting = working_dir.rights_inheriting;

    // ASSUMPTION: open rights apply recursively
    if !working_dir.rights.contains(Rights::PATH_OPEN) {
        return Errno::Access;
    }

    let path_string = unsafe { get_input_str!(&memory, path, path_len) };

    debug!("=> path_open(): fd: {}, path: {}", dirfd, &path_string);

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
    //
    // Maximum rights: should be the working dir rights
    // Minimum rights: whatever rights are provided
    let adjusted_rights = /*fs_rights_base &*/ working_dir_rights_inheriting;
    let mut open_options = state.fs_new_open_options();

    let target_rights = match maybe_inode {
        Ok(_) => {
            let write_permission = adjusted_rights.contains(Rights::FD_WRITE);

            // append, truncate, and create all require the permission to write
            let (append_permission, truncate_permission, create_permission) = if write_permission {
                (
                    fs_flags.contains(Fdflags::APPEND),
                    o_flags.contains(Oflags::TRUNC),
                    o_flags.contains(Oflags::CREATE),
                )
            } else {
                (false, false, false)
            };

            wasmer_vfs::OpenOptionsConfig {
                read: fs_rights_base.contains(Rights::FD_READ),
                write: write_permission,
                create_new: create_permission && o_flags.contains(Oflags::EXCL),
                create: create_permission,
                append: append_permission,
                truncate: truncate_permission,
            }
        }
        Err(_) => wasmer_vfs::OpenOptionsConfig {
            append: fs_flags.contains(Fdflags::APPEND),
            write: fs_rights_base.contains(Rights::FD_WRITE),
            read: fs_rights_base.contains(Rights::FD_READ),
            create_new: o_flags.contains(Oflags::CREATE) && o_flags.contains(Oflags::EXCL),
            create: o_flags.contains(Oflags::CREATE),
            truncate: o_flags.contains(Oflags::TRUNC),
        },
    };

    let parent_rights = wasmer_vfs::OpenOptionsConfig {
        read: working_dir.rights.contains(Rights::FD_READ),
        write: working_dir.rights.contains(Rights::FD_WRITE),
        // The parent is a directory, which is why these options
        // aren't inherited from the parent (append / truncate doesn't work on directories)
        create_new: true,
        create: true,
        append: true,
        truncate: true,
    };

    let minimum_rights = target_rights.minimum_rights(&parent_rights);

    open_options.options(minimum_rights.clone());

    let inode = if let Ok(inode) = maybe_inode {
        // Happy path, we found the file we're trying to open
        let mut guard = inodes.arena[inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::File {
                ref mut handle,
                path,
                fd,
            } => {
                if let Some(special_fd) = fd {
                    // short circuit if we're dealing with a special file
                    assert!(handle.is_some());
                    wasi_try_mem!(fd_ref.write(*special_fd));
                    return Errno::Success;
                }
                if o_flags.contains(Oflags::DIRECTORY) {
                    return Errno::Notdir;
                }
                if o_flags.contains(Oflags::EXCL) {
                    return Errno::Exist;
                }

                let open_options = open_options
                    .write(minimum_rights.write)
                    .create(minimum_rights.create)
                    .append(minimum_rights.append)
                    .truncate(minimum_rights.truncate);

                if minimum_rights.read {
                    open_flags |= Fd::READ;
                }
                if minimum_rights.write {
                    open_flags |= Fd::WRITE;
                }
                if minimum_rights.create {
                    open_flags |= Fd::CREATE;
                }
                if minimum_rights.truncate {
                    open_flags |= Fd::TRUNCATE;
                }

                *handle = Some(wasi_try!(open_options
                    .open(&path)
                    .map_err(fs_error_into_wasi_err)));
            }
            Kind::Buffer { .. } => unimplemented!("wasi::path_open for Buffer type files"),
            Kind::Root { .. } => {
                if !o_flags.contains(Oflags::DIRECTORY) {
                    return Errno::Notcapable;
                }
            }
            Kind::Dir { .. }
            | Kind::Socket { .. }
            | Kind::Pipe { .. }
            | Kind::EventNotifications { .. } => {}
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
        if o_flags.contains(Oflags::CREATE) {
            if o_flags.contains(Oflags::DIRECTORY) {
                return Errno::Notdir;
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
                let deref = guard.deref();
                match deref {
                    Kind::Dir { path, .. } => {
                        let mut new_path = path.clone();
                        new_path.push(&new_entity_name);
                        new_path
                    }
                    Kind::Root { .. } => {
                        let mut new_path = std::path::PathBuf::new();
                        new_path.push(&new_entity_name);
                        new_path
                    }
                    _ => return Errno::Inval,
                }
            };
            // once we got the data we need from the parent, we lookup the host file
            // todo: extra check that opening with write access is okay
            let handle = {
                let open_options = open_options
                    .read(minimum_rights.read)
                    .append(minimum_rights.append)
                    .write(minimum_rights.write)
                    .create_new(minimum_rights.create_new);

                if minimum_rights.read {
                    open_flags |= Fd::READ;
                }
                if minimum_rights.write {
                    open_flags |= Fd::WRITE;
                }
                if minimum_rights.create_new {
                    open_flags |= Fd::CREATE;
                }
                if minimum_rights.truncate {
                    open_flags |= Fd::TRUNCATE;
                }

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

    Errno::Success
}

/// ### `path_readlink()`
/// Read the value of a symlink
/// Inputs:
/// - `Fd dir_fd`
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
pub fn path_readlink<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    dir_fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    buf_used: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi::path_readlink");
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(dir_fd));
    if !base_dir.rights.contains(Rights::PATH_READLINK) {
        return Errno::Access;
    }
    let path_str = unsafe { get_input_str!(&memory, path, path_len) };
    let inode = wasi_try!(state
        .fs
        .get_inode_at_path(inodes.deref_mut(), dir_fd, &path_str, false));

    {
        let guard = inodes.arena[inode].read();
        if let Kind::Symlink { relative_path, .. } = guard.deref() {
            let rel_path_str = relative_path.to_string_lossy();
            debug!("Result => {:?}", rel_path_str);
            let buf_len: u64 = buf_len.into();
            let bytes = rel_path_str.bytes();
            if bytes.len() as u64 >= buf_len {
                return Errno::Overflow;
            }
            let bytes: Vec<_> = bytes.collect();

            let out = wasi_try_mem!(buf.slice(&memory, wasi_try!(to_offset::<M>(bytes.len()))));
            wasi_try_mem!(out.write_slice(&bytes));
            // should we null terminate this?

            let bytes_len: M::Offset =
                wasi_try!(bytes.len().try_into().map_err(|_| Errno::Overflow));
            wasi_try_mem!(buf_used.deref(&memory).write(bytes_len));
        } else {
            return Errno::Inval;
        }
    }

    Errno::Success
}

/// Returns Errno::Notemtpy if directory is not empty
pub fn path_remove_directory<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    // TODO check if fd is a dir, ensure it's within sandbox, etc.
    debug!("wasi::path_remove_directory");
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    let path_str = unsafe { get_input_str!(&memory, path, path_len) };

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
        let deref = guard.deref();
        match deref {
            Kind::Dir { entries, path, .. } => {
                if !entries.is_empty() || wasi_try!(state.fs_read_dir(path)).count() != 0 {
                    return Errno::Notempty;
                }
                path.clone()
            }
            Kind::Root { .. } => return Errno::Access,
            _ => return Errno::Notdir,
        }
    };

    {
        let mut guard = inodes.arena[parent_inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::Dir {
                ref mut entries, ..
            } => {
                let removed_inode = wasi_try!(entries.remove(&childs_name).ok_or(Errno::Inval));
                // TODO: make this a debug assert in the future
                assert!(inode == removed_inode);
            }
            Kind::Root { .. } => return Errno::Access,
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

    Errno::Success
}

/// ### `path_rename()`
/// Rename a file or directory
/// Inputs:
/// - `Fd old_fd`
///     The base directory for `old_path`
/// - `const char* old_path`
///     Pointer to UTF8 bytes, the file to be renamed
/// - `u32 old_path_len`
///     The number of bytes to read from `old_path`
/// - `Fd new_fd`
///     The base directory for `new_path`
/// - `const char* new_path`
///     Pointer to UTF8 bytes, the new file name
/// - `u32 new_path_len`
///     The number of bytes to read from `new_path`
pub fn path_rename<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    old_fd: WasiFd,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    new_fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Errno {
    debug!(
        "wasi::path_rename: old_fd = {}, new_fd = {}",
        old_fd, new_fd
    );
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
    let source_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    let source_path = std::path::Path::new(&source_str);
    let target_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
    let target_path = std::path::Path::new(&target_str);
    debug!("=> rename from {} to {}", &source_str, &target_str);

    {
        let source_fd = wasi_try!(state.fs.get_fd(old_fd));
        if !source_fd.rights.contains(Rights::PATH_RENAME_SOURCE) {
            return Errno::Access;
        }
        let target_fd = wasi_try!(state.fs.get_fd(new_fd));
        if !target_fd.rights.contains(Rights::PATH_RENAME_TARGET) {
            return Errno::Access;
        }
    }

    // this is to be sure the source file is fetch from filesystem if needed
    wasi_try!(state.fs.get_inode_at_path(
        inodes.deref_mut(),
        old_fd,
        source_path.to_str().as_ref().unwrap(),
        true
    ));
    // Create the destination inode if the file exists.
    let _ = state.fs.get_inode_at_path(
        inodes.deref_mut(),
        new_fd,
        target_path.to_str().as_ref().unwrap(),
        true,
    );
    let (source_parent_inode, source_entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), old_fd, source_path, true));
    let (target_parent_inode, target_entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), new_fd, target_path, true));
    let mut need_create = true;
    let host_adjusted_target_path = {
        let guard = inodes.arena[target_parent_inode].read();
        let deref = guard.deref();
        match deref {
            Kind::Dir { entries, path, .. } => {
                if entries.contains_key(&target_entry_name) {
                    need_create = false;
                }
                let mut out_path = path.clone();
                out_path.push(std::path::Path::new(&target_entry_name));
                out_path
            }
            Kind::Root { .. } => return Errno::Notcapable,
            Kind::Socket { .. } | Kind::Pipe { .. } | Kind::EventNotifications { .. } => {
                return Errno::Inval
            }
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                unreachable!("Fatal internal logic error: parent of inode is not a directory")
            }
        }
    };

    let source_entry = {
        let mut guard = inodes.arena[source_parent_inode].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::Dir { entries, .. } => {
                wasi_try!(entries.remove(&source_entry_name).ok_or(Errno::Noent))
            }
            Kind::Root { .. } => return Errno::Notcapable,
            Kind::Socket { .. } | Kind::Pipe { .. } | Kind::EventNotifications { .. } => {
                return Errno::Inval
            }
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                unreachable!("Fatal internal logic error: parent of inode is not a directory")
            }
        }
    };

    {
        let mut guard = inodes.arena[source_entry].write();
        let deref_mut = guard.deref_mut();
        match deref_mut {
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
                if let Err(e) = result {
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
            Kind::Socket { .. } => {}
            Kind::Pipe { .. } => {}
            Kind::EventNotifications { .. } => {}
            Kind::Root { .. } => unreachable!("The root can not be moved"),
        }
    }

    if need_create {
        let mut guard = inodes.arena[target_parent_inode].write();
        if let Kind::Dir { entries, .. } = guard.deref_mut() {
            let result = entries.insert(target_entry_name, source_entry);
            assert!(
                result.is_none(),
                "Fatal error: race condition on filesystem detected or internal logic error"
            );
        }
    }

    Errno::Success
}

/// ### `path_symlink()`
/// Create a symlink
/// Inputs:
/// - `const char *old_path`
///     Array of UTF-8 bytes representing the source path
/// - `u32 old_path_len`
///     The number of bytes to read from `old_path`
/// - `Fd fd`
///     The base directory from which the paths are understood
/// - `const char *new_path`
///     Array of UTF-8 bytes representing the target path
/// - `u32 new_path_len`
///     The number of bytes to read from `new_path`
pub fn path_symlink<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Errno {
    debug!("wasi::path_symlink");
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
    let old_path_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    let new_path_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
    let base_fd = wasi_try!(state.fs.get_fd(fd));
    if !base_fd.rights.contains(Rights::PATH_SYMLINK) {
        return Errno::Access;
    }

    // get the depth of the parent + 1 (UNDER INVESTIGATION HMMMMMMMM THINK FISH ^ THINK FISH)
    let old_path_path = std::path::Path::new(&old_path_str);
    let (source_inode, _) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), fd, old_path_path, true));
    let depth = state
        .fs
        .path_depth_from_fd(inodes.deref(), fd, source_inode);

    // depth == -1 means folder is not relative. See issue #3233.
    let depth = match depth {
        Ok(depth) => depth as i32 - 1,
        Err(_) => -1,
    };

    let new_path_path = std::path::Path::new(&new_path_str);
    let (target_parent_inode, entry_name) =
        wasi_try!(state
            .fs
            .get_parent_inode_at_path(inodes.deref_mut(), fd, new_path_path, true));

    // short circuit if anything is wrong, before we create an inode
    {
        let guard = inodes.arena[target_parent_inode].read();
        let deref = guard.deref();
        match deref {
            Kind::Dir { entries, .. } => {
                if entries.contains_key(&entry_name) {
                    return Errno::Exist;
                }
            }
            Kind::Root { .. } => return Errno::Notcapable,
            Kind::Socket { .. } | Kind::Pipe { .. } | Kind::EventNotifications { .. } => {
                return Errno::Inval
            }
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

    Errno::Success
}

/// ### `path_unlink_file()`
/// Unlink a file, deleting if the number of hardlinks is 1
/// Inputs:
/// - `Fd fd`
///     The base file descriptor from which the path is understood
/// - `const char *path`
///     Array of UTF-8 bytes representing the path
/// - `u32 path_len`
///     The number of bytes in the `path` array
pub fn path_unlink_file<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    debug!("wasi::path_unlink_file");
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    if !base_dir.rights.contains(Rights::PATH_UNLINK_FILE) {
        return Errno::Access;
    }
    let path_str = unsafe { get_input_str!(&memory, path, path_len) };
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
        let deref_mut = guard.deref_mut();
        match deref_mut {
            Kind::Dir {
                ref mut entries, ..
            } => {
                let removed_inode = wasi_try!(entries.remove(&childs_name).ok_or(Errno::Inval));
                // TODO: make this a debug assert in the future
                assert!(inode == removed_inode);
                debug_assert!(inodes.arena[inode].stat.read().unwrap().st_nlink > 0);
                removed_inode
            }
            Kind::Root { .. } => return Errno::Access,
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
            let deref_mut = guard.deref_mut();
            match deref_mut {
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
                Kind::Dir { .. } | Kind::Root { .. } => return Errno::Isdir,
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

    Errno::Success
}

/// ### `poll_oneoff()`
/// Concurrently poll for a set of events
/// Inputs:
/// - `const __wasi_subscription_t *in`
///     The events to subscribe to
/// - `Event *out`
///     The events that have occured
/// - `u32 nsubscriptions`
///     The number of subscriptions and the number of events
/// Output:
/// - `u32 nevents`
///     The number of events seen
pub fn poll_oneoff<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    in_: WasmPtr<Subscription, M>,
    out_: WasmPtr<Event, M>,
    nsubscriptions: M::Offset,
    nevents: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!("wasi::poll_oneoff");
    trace!("  => nsubscriptions = {}", nsubscriptions);
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let subscription_array = wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions));
    let event_array = wasi_try_mem_ok!(out_.slice(&memory, nsubscriptions));
    let mut events_seen: u32 = 0;
    let out_ptr = nevents.deref(&memory);

    let mut fd_guards = vec![];
    let mut clock_subs = vec![];
    let mut in_events = vec![];
    let mut time_to_sleep = Duration::from_millis(5);

    for sub in subscription_array.iter() {
        let s: Subscription = wasi_try_mem_ok!(sub.read());
        let mut peb = PollEventBuilder::new();

        let fd = match s.data {
            SubscriptionEnum::Read(SubscriptionFsReadwrite { file_descriptor }) => {
                match file_descriptor {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    _ => {
                        let fd_entry = wasi_try_ok!(state.fs.get_fd(file_descriptor), env);
                        if !fd_entry.rights.contains(Rights::FD_READ) {
                            return Ok(Errno::Access);
                        }
                    }
                }
                in_events.push(peb.add(PollEvent::PollIn).build());
                Some(file_descriptor)
            }
            SubscriptionEnum::Write(SubscriptionFsReadwrite { file_descriptor }) => {
                match file_descriptor {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    _ => {
                        let fd_entry = wasi_try_ok!(state.fs.get_fd(file_descriptor), env);
                        if !fd_entry.rights.contains(Rights::FD_WRITE) {
                            return Ok(Errno::Access);
                        }
                    }
                }
                in_events.push(peb.add(PollEvent::PollOut).build());
                Some(file_descriptor)
            }
            SubscriptionEnum::Clock(clock_info) => {
                if matches!(clock_info.clock_id, Clockid::Realtime | Clockid::Monotonic) {
                    // this is a hack
                    // TODO: do this properly
                    time_to_sleep = Duration::from_nanos(clock_info.timeout);
                    clock_subs.push((clock_info, s.userdata));
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
                        env
                    )
                }
                __WASI_STDIN_FILENO => {
                    wasi_try_ok!(
                        inodes
                            .stdin(&state.fs.fd_map)
                            .map_err(fs_error_into_wasi_err),
                        env
                    )
                }
                __WASI_STDOUT_FILENO => {
                    wasi_try_ok!(
                        inodes
                            .stdout(&state.fs.fd_map)
                            .map_err(fs_error_into_wasi_err),
                        env
                    )
                }
                _ => {
                    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd), env);
                    let inode = fd_entry.inode;
                    if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                        return Ok(Errno::Access);
                    }

                    {
                        let guard = inodes.arena[inode].read();
                        let deref = guard.deref();
                        match deref {
                            Kind::File { handle, .. } => {
                                if let Some(h) = handle {
                                    crate::state::InodeValFileReadGuard { guard }
                                } else {
                                    return Ok(Errno::Badf);
                                }
                            }
                            Kind::Socket { .. }
                            | Kind::Pipe { .. }
                            | Kind::EventNotifications { .. } => {
                                return Ok(Errno::Badf);
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

    #[allow(clippy::significant_drop_in_scrutinee)]
    let fds = {
        let mut f = vec![];
        for fd in fd_guards.iter() {
            f.push(wasi_try_ok!(fd.as_ref().ok_or(Errno::Badf)).deref());
        }
        f
    };

    let mut seen_events = vec![Default::default(); in_events.len()];

    let start = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
    let mut triggered = 0;
    while triggered == 0 {
        let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
        let delta = match now.checked_sub(start) {
            Some(a) => Duration::from_nanos(a as u64),
            None => Duration::ZERO,
        };
        match poll(
            fds.as_slice(),
            in_events.as_slice(),
            seen_events.as_mut_slice(),
            Duration::from_millis(1),
        ) {
            Ok(0) => {
                env.yield_now()?;
            }
            Ok(a) => {
                triggered = a;
            }
            Err(FsError::WouldBlock) => {
                env.sleep(Duration::from_millis(1))?;
            }
            Err(err) => {
                return Ok(fs_error_into_wasi_err(err));
            }
        };
        if delta > time_to_sleep {
            break;
        }
    }

    for (i, seen_event) in seen_events.into_iter().enumerate() {
        let mut flags = Eventrwflags::empty();
        let mut error = Errno::Again;
        let mut bytes_available = 0;
        let event_iter = iterate_poll_events(seen_event);
        for event in event_iter {
            match event {
                PollEvent::PollError => error = Errno::Io,
                PollEvent::PollHangUp => flags = Eventrwflags::FD_READWRITE_HANGUP,
                PollEvent::PollInvalid => error = Errno::Inval,
                PollEvent::PollIn => {
                    bytes_available = wasi_try_ok!(
                        fds[i]
                            .bytes_available_read()
                            .map_err(fs_error_into_wasi_err),
                        env
                    )
                    .unwrap_or(0usize);
                    error = Errno::Success;
                }
                PollEvent::PollOut => {
                    bytes_available = wasi_try_ok!(
                        fds[i]
                            .bytes_available_write()
                            .map_err(fs_error_into_wasi_err),
                        env
                    )
                    .unwrap_or(0usize);
                    error = Errno::Success;
                }
            }
        }
        let event = Event {
            userdata: wasi_try_mem_ok!(subscription_array.index(i as u64).read()).userdata,
            error,
            data: match wasi_try_mem_ok!(subscription_array.index(i as u64).read()).data {
                SubscriptionEnum::Read(d) => EventEnum::FdRead(EventFdReadwrite {
                    nbytes: bytes_available as u64,
                    flags,
                }),
                SubscriptionEnum::Write(d) => EventEnum::FdWrite(EventFdReadwrite {
                    nbytes: bytes_available as u64,
                    flags,
                }),
                SubscriptionEnum::Clock(_) => EventEnum::Clock,
            },
        };
        wasi_try_mem_ok!(event_array.index(events_seen as u64).write(event));
        events_seen += 1;
    }
    if triggered == 0 {
        for (clock_info, userdata) in clock_subs {
            let event = Event {
                userdata,
                error: Errno::Success,
                data: EventEnum::Clock,
            };
            wasi_try_mem_ok!(event_array.index(events_seen as u64).write(event));
            events_seen += 1;
        }
    }
    let events_seen: M::Offset = wasi_try_ok!(events_seen.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(out_ptr.write(events_seen));
    Ok(Errno::Success)
}

/// ### `proc_exit()`
/// Terminate the process normally. An exit code of 0 indicates successful
/// termination of the program. The meanings of other values is dependent on
/// the environment.
/// Inputs:
/// - `__wasi_exitcode_t`
///   Exit code to return to the operating system
pub fn proc_exit(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    code: __wasi_exitcode_t,
) -> Result<(), WasiError> {
    debug!("wasi::proc_exit, {}", code);
    Err(WasiError::Exit(code))
}

/// ### `proc_raise()`
/// Send a signal to the process of the calling thread.
/// Note: This is similar to `raise` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
pub fn proc_raise(ctx: FunctionEnvMut<'_, WasiEnv>, sig: Signal) -> Errno {
    debug!("wasi::proc_raise");
    unimplemented!("wasi::proc_raise")
}

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    trace!("wasi::sched_yield");
    let env = ctx.data();
    env.yield_now()?;
    Ok(Errno::Success)
}

/// ### `random_get()`
/// Fill buffer with high-quality random data.  This function may be slow and block
/// Inputs:
/// - `void *buf`
///     A pointer to a buffer where the random bytes will be written
/// - `size_t buf_len`
///     The number of bytes that will be written
pub fn random_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
) -> Errno {
    trace!("wasi::random_get buf_len: {}", buf_len);
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let buf_len64: u64 = buf_len.into();
    let mut u8_buffer = vec![0; buf_len64 as usize];
    let res = getrandom::getrandom(&mut u8_buffer);
    match res {
        Ok(()) => {
            let buf = wasi_try_mem!(buf.slice(&memory, buf_len));
            wasi_try_mem!(buf.write_slice(&u8_buffer));
            Errno::Success
        }
        Err(_) => Errno::Io,
    }
}

/// ### `tty_get()`
/// Retrieves the current state of the TTY
pub fn tty_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    tty_state: WasmPtr<Tty, M>,
) -> Errno {
    debug!("wasi::tty_stdin");
    let env = ctx.data();

    let state = env.runtime.tty_get();
    let state = Tty {
        cols: state.cols,
        rows: state.rows,
        width: state.width,
        height: state.height,
        stdin_tty: state.stdin_tty,
        stdout_tty: state.stdout_tty,
        stderr_tty: state.stderr_tty,
        echo: state.echo,
        line_buffered: state.line_buffered,
    };

    let memory = env.memory_view(&ctx);
    wasi_try_mem!(tty_state.write(&memory, state));

    Errno::Success
}

/// ### `tty_set()`
/// Updates the properties of the rect
pub fn tty_set<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    tty_state: WasmPtr<Tty, M>,
) -> Errno {
    debug!("wasi::tty_set");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let state = wasi_try_mem!(tty_state.read(&memory));
    let state = super::runtime::WasiTtyState {
        cols: state.cols,
        rows: state.rows,
        width: state.width,
        height: state.height,
        stdin_tty: state.stdin_tty,
        stdout_tty: state.stdout_tty,
        stderr_tty: state.stderr_tty,
        echo: state.echo,
        line_buffered: state.line_buffered,
    };

    env.runtime.tty_set(state);

    Errno::Success
}

/// ### `getcwd()`
/// Returns the current working directory
/// If the path exceeds the size of the buffer then this function
/// will fill the path_len with the needed size and return EOVERFLOW
pub fn getcwd<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi::getcwd");
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let (_, cur_dir) = wasi_try!(state
        .fs
        .get_current_dir(inodes.deref_mut(), crate::VIRTUAL_ROOT_FD,));

    let max_path_len = wasi_try_mem!(path_len.read(&memory));
    let path_slice = wasi_try_mem!(path.slice(&memory, max_path_len));
    let max_path_len: u64 = max_path_len.into();

    let cur_dir = cur_dir.as_bytes();
    wasi_try_mem!(path_len.write(&memory, wasi_try!(to_offset::<M>(cur_dir.len()))));
    if cur_dir.len() as u64 >= max_path_len {
        return Errno::Overflow;
    }

    let cur_dir = {
        let mut u8_buffer = vec![0; max_path_len as usize];
        let cur_dir_len = cur_dir.len();
        if (cur_dir_len as u64) < max_path_len {
            u8_buffer[..cur_dir_len].clone_from_slice(cur_dir);
            u8_buffer[cur_dir_len] = 0;
        } else {
            return Errno::Overflow;
        }
        u8_buffer
    };

    wasi_try_mem!(path_slice.write_slice(&cur_dir[..]));
    Errno::Success
}

/// ### `chdir()`
/// Sets the current working directory
pub fn chdir<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Errno {
    debug!("wasi::chdir");
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let path = unsafe { get_input_str!(&memory, path, path_len) };

    state.fs.set_current_dir(path.as_str());
    Errno::Success
}

/// ### `thread_spawn()`
/// Creates a new thread by spawning that shares the same
/// memory address space, file handles and main event loops.
/// The function referenced by the fork call must be
/// exported by the web assembly process.
///
/// ## Parameters
///
/// * `name` - Name of the function that will be invoked as a new thread
/// * `user_data` - User data that will be supplied to the function when its called
/// * `reactor` - Indicates if the function will operate as a reactor or
///   as a normal thread. Reactors will be repeatable called
///   whenever IO work is available to be processed.
///
/// ## Return
///
/// Returns the thread index of the newly created thread
/// (indices always start from zero)
pub fn thread_spawn<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    method: WasmPtr<u8, M>,
    method_len: M::Offset,
    user_data: u64,
    reactor: Bool,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    debug!("wasi::thread_spawn");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let method = unsafe { get_input_str!(&memory, method, method_len) };

    // Load the callback function
    if method.as_str() != "_thread_start" {
        return Errno::Notcapable;
    };
    /*
    let funct = unsafe {
        if env.thread_start_ref().is_none() {
            return Errno::Addrnotavail;
        }
        env.thread_start_ref_unchecked()
    };
    */

    let reactor = match reactor {
        Bool::False => false,
        Bool::True => true,
        _ => return Errno::Inval,
    };

    // Create the sub-thread
    let mut sub_env = env.clone();
    let mut sub_thread = env.new_thread();
    sub_env.id = sub_thread.id;

    let child = {
        let id = sub_thread.id;
        wasi_try!(env
            .runtime
            .thread_spawn(Box::new(move || {
                /*
                if let Some(funct) = sub_env.thread_start_ref() {
                    if let Err(err) = funct.call(user_data) {
                        warn!("thread failed: {}", err);
                        std::mem::forget(sub_thread);
                        return;
                    }
                } else {
                    warn!("failed to start thread: missing callback '__wasix_thread_start'");
                    std::mem::forget(sub_thread);
                    return;
                }
                */

                let thread = {
                    let mut guard = sub_env.state.threading.lock().unwrap();
                    let thread = guard.threads.remove(&id);
                    drop(guard);
                    thread
                };

                if let Some(thread) = thread {
                    let mut thread_guard = thread.exit.lock().unwrap();
                    thread_guard.take();
                }
                drop(sub_thread);
            }))
            .map_err(|err| {
                let err: Errno = err.into();
                err
            }));
        id
    };
    let child: Tid = child.into();

    wasi_try_mem!(ret_tid.write(&memory, child));
    Errno::Success
}

/// ### `thread_sleep()`
/// Sends the current thread to sleep for a period of time
///
/// ## Parameters
///
/// * `duration` - Amount of time that the thread should sleep
pub fn thread_sleep(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    duration: Timestamp,
) -> Result<Errno, WasiError> {
    debug!("wasi::thread_sleep");

    let env = ctx.data();
    let duration = Duration::from_nanos(duration as u64);
    env.sleep(duration)?;
    Ok(Errno::Success)
}

/// ### `thread_id()`
/// Returns the index of the current thread
/// (threads indices are sequencial from zero)
pub fn thread_id<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    debug!("wasi::thread_id");

    let env = ctx.data();
    let tid: Tid = env.id.into();
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_tid.write(&memory, tid));
    Errno::Success
}

/// ### `thread_join()`
/// Joins this thread with another thread, blocking this
/// one until the other finishes
///
/// ## Parameters
///
/// * `tid` - Handle of the thread to wait on
pub fn thread_join(ctx: FunctionEnvMut<'_, WasiEnv>, tid: Tid) -> Result<Errno, WasiError> {
    debug!("wasi::thread_join");

    let env = ctx.data();
    let tid: WasiThreadId = tid.into();
    let other_thread = {
        let guard = env.state.threading.lock().unwrap();
        guard.threads.get(&tid).cloned()
    };
    if let Some(other_thread) = other_thread {
        loop {
            if other_thread.join(Duration::from_millis(5)) {
                break;
            }
            env.yield_now()?;
        }
        Ok(Errno::Success)
    } else {
        Ok(Errno::Success)
    }
}

/// ### `thread_parallelism()`
/// Returns the available parallelism which is normally the
/// number of available cores that can run concurrently
pub fn thread_parallelism<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_parallelism: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi::thread_parallelism");

    let env = ctx.data();
    let parallelism = wasi_try!(env.runtime().thread_parallelism().map_err(|err| {
        let err: Errno = err.into();
        err
    }));
    let parallelism: M::Offset = wasi_try!(parallelism.try_into().map_err(|_| Errno::Overflow));
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_parallelism.write(&memory, parallelism));
    Errno::Success
}

/// ### `getpid()`
/// Returns the handle of the current process
pub fn getpid<M: MemorySize>(ctx: FunctionEnvMut<'_, WasiEnv>, ret_pid: WasmPtr<Pid, M>) -> Errno {
    debug!("wasi::getpid");

    let env = ctx.data();
    let pid = env.runtime().getpid();
    if let Some(pid) = pid {
        let memory = env.memory_view(&ctx);
        wasi_try_mem!(ret_pid.write(&memory, pid as Pid));
        Errno::Success
    } else {
        Errno::Notsup
    }
}

/// ### `thread_exit()`
/// Terminates the current running thread, if this is the last thread then
/// the process will also exit with the specified exit code. An exit code
/// of 0 indicates successful termination of the thread. The meanings of
/// other values is dependent on the environment.
///
/// ## Parameters
///
/// * `rval` - The exit code returned by the process.
pub fn thread_exit(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    exitcode: __wasi_exitcode_t,
) -> Result<Errno, WasiError> {
    debug!("wasi::thread_exit");
    Err(WasiError::Exit(exitcode))
}

/// Spawns a new process within the context of this machine
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `chroot` - Indicates if the process will chroot or not
/// * `args` - List of the arguments to pass the process
///   (entries are separated by line feeds)
/// * `preopen` - List of the preopens for this process
///   (entries are separated by line feeds)
/// * `stdin` - How will stdin be handled
/// * `stdout` - How will stdout be handled
/// * `stderr` - How will stderr be handled
/// * `working_dir` - Working directory where this process should run
///   (passing '.' will use the current directory)
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
pub fn process_spawn<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    chroot: Bool,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
    preopen: WasmPtr<u8, M>,
    preopen_len: M::Offset,
    stdin: WasiStdioMode,
    stdout: WasiStdioMode,
    stderr: WasiStdioMode,
    working_dir: WasmPtr<u8, M>,
    working_dir_len: M::Offset,
    ret_handles: WasmPtr<BusHandles, M>,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus!(&memory, name, name_len) };
    let args = unsafe { get_input_str_bus!(&memory, args, args_len) };
    let preopen = unsafe { get_input_str_bus!(&memory, preopen, preopen_len) };
    let working_dir = unsafe { get_input_str_bus!(&memory, working_dir, working_dir_len) };
    let chroot = chroot == Bool::True;
    debug!("wasi::process_spawn (name={})", name);

    let args: Vec<_> = args.split(&['\n', '\r']).map(|a| a.to_string()).collect();

    let preopen: Vec<_> = preopen
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .collect();

    let conv_stdio_mode = |mode: WasiStdioMode| match mode {
        WasiStdioMode::Piped => StdioMode::Piped,
        WasiStdioMode::Inherit => StdioMode::Inherit,
        WasiStdioMode::Log => StdioMode::Log,
        /*__WASI_STDIO_MODE_NULL |*/ _ => StdioMode::Null,
    };

    let process = wasi_try_bus!(bus
        .new_spawn()
        .chroot(chroot)
        .args(args)
        .preopen(preopen)
        .stdin_mode(conv_stdio_mode(stdin))
        .stdout_mode(conv_stdio_mode(stdout))
        .stderr_mode(conv_stdio_mode(stderr))
        .working_dir(working_dir)
        .spawn(name.as_str())
        .map_err(bus_error_into_wasi_err));

    let conv_stdio_fd = |a: Option<FileDescriptor>| match a {
        Some(fd) => OptionFd {
            tag: OptionTag::Some,
            fd: fd.into(),
        },
        None => OptionFd {
            tag: OptionTag::None,
            fd: 0,
        },
    };

    // Convert the stdio
    let stdin = conv_stdio_fd(process.inst.stdin_fd());
    let stdout = conv_stdio_fd(process.inst.stdout_fd());
    let stderr = conv_stdio_fd(process.inst.stderr_fd());

    // Add the process to the environment state
    let bid = {
        let mut guard = env.state.threading.lock().unwrap();
        guard.process_seed += 1;
        let bid = guard.process_seed;
        guard.processes.insert(bid.into(), process);
        bid
    };

    let handles = BusHandles {
        bid,
        stdin,
        stdout,
        stderr,
    };

    wasi_try_mem_bus!(ret_handles.write(&memory, handles));

    BusErrno::Success
}

/// Spawns a new bus process for a particular web WebAssembly
/// binary that is referenced by its process name.
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `reuse` - Indicates if the existing processes should be reused
///   if they are already running
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
pub fn bus_open_local<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    reuse: Bool,
    ret_bid: WasmPtr<Bid, M>,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus!(&memory, name, name_len) };
    let reuse = reuse == Bool::True;
    debug!("wasi::bus_open_local (name={}, reuse={})", name, reuse);

    bus_open_local_internal(ctx, name, reuse, None, None, ret_bid)
}

/// Spawns a new bus process for a particular web WebAssembly
/// binary that is referenced by its process name on a remote instance.
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `reuse` - Indicates if the existing processes should be reused
///   if they are already running
/// * `instance` - Instance identifier where this process will be spawned
/// * `token` - Acceess token used to authenticate with the instance
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
pub fn bus_open_remote<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    reuse: Bool,
    instance: WasmPtr<u8, M>,
    instance_len: M::Offset,
    token: WasmPtr<u8, M>,
    token_len: M::Offset,
    ret_bid: WasmPtr<Bid, M>,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus!(&memory, name, name_len) };
    let instance = unsafe { get_input_str_bus!(&memory, instance, instance_len) };
    let token = unsafe { get_input_str_bus!(&memory, token, token_len) };
    let reuse = reuse == Bool::True;
    debug!(
        "wasi::bus_open_remote (name={}, reuse={}, instance={})",
        name, reuse, instance
    );

    bus_open_local_internal(ctx, name, reuse, Some(instance), Some(token), ret_bid)
}

fn bus_open_local_internal<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    name: String,
    reuse: bool,
    instance: Option<String>,
    token: Option<String>,
    ret_bid: WasmPtr<Bid, M>,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name: Cow<'static, str> = name.into();

    // Check if it already exists
    if reuse {
        let guard = env.state.threading.lock().unwrap();
        if let Some(bid) = guard.process_reuse.get(&name) {
            if guard.processes.contains_key(bid) {
                wasi_try_mem_bus!(ret_bid.write(&memory, (*bid).into()));
                return BusErrno::Success;
            }
        }
    }

    let mut process = bus.new_spawn();
    process
        .reuse(reuse)
        .stdin_mode(StdioMode::Null)
        .stdout_mode(StdioMode::Null)
        .stderr_mode(StdioMode::Log);

    if let Some(instance) = instance {
        process.remote_instance(instance);
    }

    if let Some(token) = token {
        process.access_token(token);
    }

    let process = wasi_try_bus!(process
        .spawn(name.as_ref())
        .map_err(bus_error_into_wasi_err));

    // Add the process to the environment state
    let bid = {
        let mut guard = env.state.threading.lock().unwrap();
        guard.process_seed += 1;
        let bid: WasiBusProcessId = guard.process_seed.into();
        guard.processes.insert(bid, process);
        guard.process_reuse.insert(name, bid);
        bid
    };

    wasi_try_mem_bus!(ret_bid.write(&memory, bid.into()));

    BusErrno::Success
}

/// Closes a bus process and releases all associated resources
///
/// ## Parameters
///
/// * `bid` - Handle of the bus process handle to be closed
pub fn bus_close(ctx: FunctionEnvMut<'_, WasiEnv>, bid: Bid) -> BusErrno {
    trace!("wasi::bus_close (bid={})", bid);
    let bid: WasiBusProcessId = bid.into();

    let env = ctx.data();
    let mut guard = env.state.threading.lock().unwrap();
    guard.processes.remove(&bid);

    BusErrno::Unsupported
}

/// Invokes a call within a running bus process.
///
/// ## Parameters
///
/// * `bid` - Handle of the bus process to invoke the call within
/// * `keep_alive` - Causes the call handle to remain open even when A
///   reply is received. It is then the  callers responsibility
///   to invoke 'bus_drop' when they are finished with the call
/// * `topic` - Topic that describes the type of call to made
/// * `format` - Format of the data pushed onto the bus
/// * `buf` - The buffer where data to be transmitted is stored
pub fn bus_call<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    bid: Bid,
    keep_alive: Bool,
    topic: WasmPtr<u8, M>,
    topic_len: M::Offset,
    format: BusDataFormat,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    ret_cid: WasmPtr<Cid, M>,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let topic = unsafe { get_input_str_bus!(&memory, topic, topic_len) };
    let keep_alive = keep_alive == Bool::True;
    trace!(
        "wasi::bus_call (bid={}, topic={}, buf_len={})",
        bid,
        topic,
        buf_len
    );

    BusErrno::Unsupported
}

/// Invokes a call within the context of another call
///
/// ## Parameters
///
/// * `parent` - Parent bus call that this is related to
/// * `keep_alive` - Causes the call handle to remain open even when A
///   reply is received. It is then the  callers responsibility
///   to invoke 'bus_drop' when they are finished with the call
/// * `topic` - Topic that describes the type of call to made
/// * `format` - Format of the data pushed onto the bus
/// * `buf` - The buffer where data to be transmitted is stored
pub fn bus_subcall<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    parent: Cid,
    keep_alive: Bool,
    topic: WasmPtr<u8, M>,
    topic_len: M::Offset,
    format: BusDataFormat,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    ret_cid: WasmPtr<Cid, M>,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let topic = unsafe { get_input_str_bus!(&memory, topic, topic_len) };
    let keep_alive = keep_alive == Bool::True;
    trace!(
        "wasi::bus_subcall (parent={}, topic={}, buf_len={})",
        parent,
        topic,
        buf_len
    );

    BusErrno::Unsupported
}

/// Polls for any outstanding events from a particular
/// bus process by its handle
///
/// ## Parameters
///
/// * `timeout` - Timeout before the poll returns, if one passed 0
///   as the timeout then this call is non blocking.
/// * `events` - An events buffer that will hold any received bus events
/// * `malloc` - Name of the function that will be invoked to allocate memory
///   Function signature fn(u64) -> u64
///
/// ## Return
///
/// Returns the number of events that have occured
pub fn bus_poll<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    timeout: Timestamp,
    events: WasmPtr<u8, M>,
    nevents: M::Offset,
    malloc: WasmPtr<u8, M>,
    malloc_len: M::Offset,
    ret_nevents: WasmPtr<M::Offset, M>,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let malloc = unsafe { get_input_str_bus!(&memory, malloc, malloc_len) };
    trace!("wasi::bus_poll (timeout={}, malloc={})", timeout, malloc);

    BusErrno::Unsupported
}

/// Replies to a call that was made to this process
/// from another process; where 'cid' is the call context.
/// This will may also drop the handle and release any
/// associated resources (if keepalive is not set)
///
/// ## Parameters
///
/// * `cid` - Handle of the call to send a reply on
/// * `format` - Format of the data pushed onto the bus
/// * `buf` - The buffer where data to be transmitted is stored
pub fn call_reply<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    cid: Cid,
    format: BusDataFormat,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    trace!(
        "wasi::call_reply (cid={}, format={}, data_len={})",
        cid,
        format,
        buf_len
    );

    BusErrno::Unsupported
}

/// Causes a fault on a particular call that was made
/// to this process from another process; where 'bid'
/// is the callering process context.
///
/// ## Parameters
///
/// * `cid` - Handle of the call to raise a fault on
/// * `fault` - Fault to be raised on the bus
pub fn call_fault(ctx: FunctionEnvMut<'_, WasiEnv>, cid: Cid, fault: BusErrno) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    debug!("wasi::call_fault (cid={}, fault={})", cid, fault);

    BusErrno::Unsupported
}

/// Closes a bus call based on its bus call handle
///
/// ## Parameters
///
/// * `cid` - Handle of the bus call handle to be dropped
pub fn call_close(ctx: FunctionEnvMut<'_, WasiEnv>, cid: Cid) -> BusErrno {
    let env = ctx.data();
    let bus = env.runtime.bus();
    trace!("wasi::call_close (cid={})", cid);

    BusErrno::Unsupported
}

/// ### `ws_connect()`
/// Connects to a websocket at a particular network URL
///
/// ## Parameters
///
/// * `url` - URL of the web socket destination to connect to
///
/// ## Return
///
/// Returns a socket handle which is used to send and receive data
pub fn ws_connect<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    url: WasmPtr<u8, M>,
    url_len: M::Offset,
    ret_sock: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!("wasi::ws_connect");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let url = unsafe { get_input_str!(&memory, url, url_len) };

    let socket = wasi_try!(env
        .net()
        .ws_connect(url.as_str())
        .map_err(net_error_into_wasi_err));

    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::WebSocket(socket)),
    };

    let inode = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind,
        false,
        "socket".to_string(),
    );
    let rights = Rights::all_socket();
    let fd = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    wasi_try_mem!(ret_sock.write(&memory, fd));

    Errno::Success
}

/// ### `http_request()`
/// Makes a HTTP request to a remote web resource and
/// returns a socket handles that are used to send and receive data
///
/// ## Parameters
///
/// * `url` - URL of the HTTP resource to connect to
/// * `method` - HTTP method to be invoked
/// * `headers` - HTTP headers to attach to the request
///   (headers seperated by lines)
/// * `gzip` - Should the request body be compressed
///
/// ## Return
///
/// The body of the response can be streamed from the returned
/// file handle
pub fn http_request<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    url: WasmPtr<u8, M>,
    url_len: M::Offset,
    method: WasmPtr<u8, M>,
    method_len: M::Offset,
    headers: WasmPtr<u8, M>,
    headers_len: M::Offset,
    gzip: Bool,
    ret_handles: WasmPtr<HttpHandles, M>,
) -> Errno {
    debug!("wasi::http_request");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let url = unsafe { get_input_str!(&memory, url, url_len) };
    let method = unsafe { get_input_str!(&memory, method, method_len) };
    let headers = unsafe { get_input_str!(&memory, headers, headers_len) };

    let gzip = match gzip {
        Bool::False => false,
        Bool::True => true,
        _ => return Errno::Inval,
    };

    let socket = wasi_try!(env
        .net()
        .http_request(url.as_str(), method.as_str(), headers.as_str(), gzip)
        .map_err(net_error_into_wasi_err));
    let socket_req = SocketHttpRequest {
        request: socket.request,
        response: None,
        headers: None,
        status: socket.status.clone(),
    };
    let socket_res = SocketHttpRequest {
        request: None,
        response: socket.response,
        headers: None,
        status: socket.status.clone(),
    };
    let socket_hdr = SocketHttpRequest {
        request: None,
        response: None,
        headers: socket.headers,
        status: socket.status,
    };

    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind_req = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::HttpRequest(
            Mutex::new(socket_req),
            InodeHttpSocketType::Request,
        )),
    };
    let kind_res = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::HttpRequest(
            Mutex::new(socket_res),
            InodeHttpSocketType::Response,
        )),
    };
    let kind_hdr = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::HttpRequest(
            Mutex::new(socket_hdr),
            InodeHttpSocketType::Headers,
        )),
    };

    let inode_req = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_req,
        false,
        "http_request".to_string(),
    );
    let inode_res = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_res,
        false,
        "http_response".to_string(),
    );
    let inode_hdr = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_hdr,
        false,
        "http_headers".to_string(),
    );
    let rights = Rights::all_socket();

    let handles = HttpHandles {
        req: wasi_try!(state
            .fs
            .create_fd(rights, rights, Fdflags::empty(), 0, inode_req)),
        res: wasi_try!(state
            .fs
            .create_fd(rights, rights, Fdflags::empty(), 0, inode_res)),
        hdr: wasi_try!(state
            .fs
            .create_fd(rights, rights, Fdflags::empty(), 0, inode_hdr)),
    };

    wasi_try_mem!(ret_handles.write(&memory, handles));

    Errno::Success
}

/// ### `http_status()`
/// Retrieves the status of a HTTP request
///
/// ## Parameters
///
/// * `fd` - Handle of the HTTP request
/// * `status` - Pointer to a buffer that will be filled with the current
///   status of this HTTP request
pub fn http_status<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    status: WasmPtr<HttpStatus, M>,
) -> Errno {
    debug!("wasi::http_status");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ref_status = status.deref(&memory);

    let http_status = wasi_try!(__sock_actor(&ctx, sock, Rights::empty(), |socket| {
        socket.http_status()
    }));

    // Write everything else and return the status to the caller
    let status = HttpStatus {
        ok: Bool::True,
        redirect: match http_status.redirected {
            true => Bool::True,
            false => Bool::False,
        },
        size: wasi_try!(Ok(http_status.size)),
        status: http_status.status,
    };

    wasi_try_mem!(ref_status.write(status));

    Errno::Success
}

/// ### `port_bridge()`
/// Securely connects to a particular remote network
///
/// ## Parameters
///
/// * `network` - Fully qualified identifier for the network
/// * `token` - Access token used to authenticate with the network
/// * `security` - Level of encryption to encapsulate the network connection with
pub fn port_bridge<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    network: WasmPtr<u8, M>,
    network_len: M::Offset,
    token: WasmPtr<u8, M>,
    token_len: M::Offset,
    security: Streamsecurity,
) -> Errno {
    debug!("wasi::port_bridge");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let network = unsafe { get_input_str!(&memory, network, network_len) };
    let token = unsafe { get_input_str!(&memory, token, token_len) };
    let security = match security {
        Streamsecurity::Unencrypted => StreamSecurity::Unencrypted,
        Streamsecurity::AnyEncryption => StreamSecurity::AnyEncyption,
        Streamsecurity::ClassicEncryption => StreamSecurity::ClassicEncryption,
        Streamsecurity::DoubleEncryption => StreamSecurity::DoubleEncryption,
        _ => return Errno::Inval,
    };

    wasi_try!(env
        .net()
        .bridge(network.as_str(), token.as_str(), security)
        .map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_unbridge()`
/// Disconnects from a remote network
pub fn port_unbridge(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!("wasi::port_unbridge");
    let env = ctx.data();
    wasi_try!(env.net().unbridge().map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_dhcp_acquire()`
/// Acquires a set of IP addresses using DHCP
pub fn port_dhcp_acquire(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!("wasi::port_dhcp_acquire");
    let env = ctx.data();
    wasi_try!(env.net().dhcp_acquire().map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_addr_add()`
/// Adds another static address to the local port
///
/// ## Parameters
///
/// * `addr` - Address to be added
pub fn port_addr_add<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_cidr_t, M>,
) -> Errno {
    debug!("wasi::port_addr_add");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let cidr = wasi_try!(super::state::read_cidr(&memory, ip));
    wasi_try!(env
        .net()
        .ip_add(cidr.ip, cidr.prefix)
        .map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_addr_remove()`
/// Removes an address from the local port
///
/// ## Parameters
///
/// * `addr` - Address to be removed
pub fn port_addr_remove<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Errno {
    debug!("wasi::port_addr_remove");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try!(super::state::read_ip(&memory, ip));
    wasi_try!(env.net().ip_remove(ip).map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_addr_clear()`
/// Clears all the addresses on the local port
pub fn port_addr_clear(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!("wasi::port_addr_clear");
    let env = ctx.data();
    wasi_try!(env.net().ip_clear().map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_mac()`
/// Returns the MAC address of the local port
pub fn port_mac<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_mac: WasmPtr<__wasi_hardwareaddress_t, M>,
) -> Errno {
    debug!("wasi::port_mac");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let mac = wasi_try!(env.net().mac().map_err(net_error_into_wasi_err));
    let mac = __wasi_hardwareaddress_t { octs: mac };
    wasi_try_mem!(ret_mac.write(&memory, mac));
    Errno::Success
}

/// ### `port_ip_list()`
/// Returns a list of all the addresses owned by the local port
/// This function fills the output buffer as much as possible.
/// If the buffer is not big enough then the naddrs address will be
/// filled with the buffer size needed and the EOVERFLOW will be returned
///
/// ## Parameters
///
/// * `addrs` - The buffer where addresses will be stored
///
/// ## Return
///
/// The number of addresses returned.
pub fn port_addr_list<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    addrs: WasmPtr<__wasi_cidr_t, M>,
    naddrs: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi::port_addr_list");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let max_addrs = wasi_try_mem!(naddrs.read(&memory));
    let max_addrs: u64 = wasi_try!(max_addrs.try_into().map_err(|_| Errno::Overflow));
    let ref_addrs =
        wasi_try_mem!(addrs.slice(&memory, wasi_try!(to_offset::<M>(max_addrs as usize))));

    let addrs = wasi_try!(env.net().ip_list().map_err(net_error_into_wasi_err));

    let addrs_len: M::Offset = wasi_try!(addrs.len().try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(naddrs.write(&memory, addrs_len));
    if addrs.len() as u64 > max_addrs {
        return Errno::Overflow;
    }

    for n in 0..addrs.len() {
        let nip = ref_addrs.index(n as u64);
        super::state::write_cidr(&memory, nip.as_ptr::<M>(), *addrs.get(n).unwrap());
    }

    Errno::Success
}

/// ### `port_gateway_set()`
/// Adds a default gateway to the port
///
/// ## Parameters
///
/// * `addr` - Address of the default gateway
pub fn port_gateway_set<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Errno {
    debug!("wasi::port_gateway_set");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try!(super::state::read_ip(&memory, ip));

    wasi_try!(env.net().gateway_set(ip).map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_route_add()`
/// Adds a new route to the local port
pub fn port_route_add<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    cidr: WasmPtr<__wasi_cidr_t, M>,
    via_router: WasmPtr<__wasi_addr_t, M>,
    preferred_until: WasmPtr<OptionTimestamp, M>,
    expires_at: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    debug!("wasi::port_route_add");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let cidr = wasi_try!(super::state::read_cidr(&memory, cidr));
    let via_router = wasi_try!(super::state::read_ip(&memory, via_router));
    let preferred_until = wasi_try_mem!(preferred_until.read(&memory));
    let preferred_until = match preferred_until.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(preferred_until.u)),
        _ => return Errno::Inval,
    };
    let expires_at = wasi_try_mem!(expires_at.read(&memory));
    let expires_at = match expires_at.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(expires_at.u)),
        _ => return Errno::Inval,
    };

    wasi_try!(env
        .net()
        .route_add(cidr, via_router, preferred_until, expires_at)
        .map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_route_remove()`
/// Removes an existing route from the local port
pub fn port_route_remove<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Errno {
    debug!("wasi::port_route_remove");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try!(super::state::read_ip(&memory, ip));
    wasi_try!(env.net().route_remove(ip).map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_route_clear()`
/// Clears all the routes in the local port
pub fn port_route_clear(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!("wasi::port_route_clear");
    let env = ctx.data();
    wasi_try!(env.net().route_clear().map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_route_list()`
/// Returns a list of all the routes owned by the local port
/// This function fills the output buffer as much as possible.
/// If the buffer is too small this will return EOVERFLOW and
/// fill nroutes with the size of the buffer needed.
///
/// ## Parameters
///
/// * `routes` - The buffer where routes will be stored
pub fn port_route_list<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    routes: WasmPtr<Route, M>,
    nroutes: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi::port_route_list");
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let nroutes = nroutes.deref(&memory);
    let max_routes: usize = wasi_try!(wasi_try_mem!(nroutes.read())
        .try_into()
        .map_err(|_| Errno::Inval));
    let ref_routes = wasi_try_mem!(routes.slice(&memory, wasi_try!(to_offset::<M>(max_routes))));

    let routes = wasi_try!(env.net().route_list().map_err(net_error_into_wasi_err));

    let routes_len: M::Offset = wasi_try!(routes.len().try_into().map_err(|_| Errno::Inval));
    wasi_try_mem!(nroutes.write(routes_len));
    if routes.len() > max_routes {
        return Errno::Overflow;
    }

    for n in 0..routes.len() {
        let nroute = ref_routes.index(n as u64);
        super::state::write_route(
            &memory,
            nroute.as_ptr::<M>(),
            routes.get(n).unwrap().clone(),
        );
    }

    Errno::Success
}

/// ### `sock_shutdown()`
/// Shut down socket send and receive channels.
/// Note: This is similar to `shutdown` in POSIX.
///
/// ## Parameters
///
/// * `how` - Which channels on the socket to shut down.
pub fn sock_shutdown(ctx: FunctionEnvMut<'_, WasiEnv>, sock: WasiFd, how: SdFlags) -> Errno {
    debug!("wasi::sock_shutdown");

    let both = __WASI_SHUT_RD | __WASI_SHUT_WR;
    let how = match how {
        __WASI_SHUT_RD => std::net::Shutdown::Read,
        __WASI_SHUT_WR => std::net::Shutdown::Write,
        a if a == both => std::net::Shutdown::Both,
        _ => return Errno::Inval,
    };

    wasi_try!(__sock_actor_mut(
        &ctx,
        sock,
        Rights::SOCK_SHUTDOWN,
        |socket| { socket.shutdown(how) }
    ));

    Errno::Success
}

/// ### `sock_status()`
/// Returns the current status of a socket
pub fn sock_status<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ret_status: WasmPtr<Sockstatus, M>,
) -> Errno {
    debug!("wasi::sock_status");

    let status = wasi_try!(__sock_actor(&ctx, sock, Rights::empty(), |socket| {
        socket.status()
    }));

    use super::state::WasiSocketStatus;
    let status = match status {
        WasiSocketStatus::Opening => Sockstatus::Opening,
        WasiSocketStatus::Opened => Sockstatus::Opened,
        WasiSocketStatus::Closed => Sockstatus::Closed,
        WasiSocketStatus::Failed => Sockstatus::Failed,
    };

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_status.write(&memory, status));

    Errno::Success
}

/// ### `sock_addr_local()`
/// Returns the local address to which the socket is bound.
///
/// Note: This is similar to `getsockname` in POSIX
///
/// When successful, the contents of the output buffer consist of an IP address,
/// either IP4 or IP6.
///
/// ## Parameters
///
/// * `fd` - Socket that the address is bound to
pub fn sock_addr_local<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ret_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!("wasi::sock_addr_local");

    let addr = wasi_try!(__sock_actor(&ctx, sock, Rights::empty(), |socket| {
        socket.addr_local()
    }));
    let memory = ctx.data().memory_view(&ctx);
    wasi_try!(super::state::write_ip_port(
        &memory,
        ret_addr,
        addr.ip(),
        addr.port()
    ));
    Errno::Success
}

/// ### `sock_addr_peer()`
/// Returns the remote address to which the socket is connected to.
///
/// Note: This is similar to `getpeername` in POSIX
///
/// When successful, the contents of the output buffer consist of an IP address,
/// either IP4 or IP6.
///
/// ## Parameters
///
/// * `fd` - Socket that the address is bound to
pub fn sock_addr_peer<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!("wasi::sock_addr_peer");

    let env = ctx.data();
    let addr = wasi_try!(__sock_actor(&ctx, sock, Rights::empty(), |socket| {
        socket.addr_peer()
    }));
    let memory = env.memory_view(&ctx);
    wasi_try!(super::state::write_ip_port(
        &memory,
        ro_addr,
        addr.ip(),
        addr.port()
    ));
    Errno::Success
}

/// ### `sock_open()`
/// Create an endpoint for communication.
///
/// creates an endpoint for communication and returns a file descriptor
/// tor that refers to that endpoint. The file descriptor returned by a successful
/// call will be the lowest-numbered file descriptor not currently open
/// for the process.
///
/// Note: This is similar to `socket` in POSIX using PF_INET
///
/// ## Parameters
///
/// * `af` - Address family
/// * `socktype` - Socket type, either datagram or stream
/// * `sock_proto` - Socket protocol
///
/// ## Return
///
/// The file descriptor of the socket that has been opened.
pub fn sock_open<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    af: Addressfamily,
    ty: Socktype,
    pt: SockProto,
    ro_sock: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!("wasi::sock_open");

    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = match ty {
        Socktype::Stream | Socktype::Dgram => Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::PreSocket {
                family: af,
                ty,
                pt,
                addr: None,
                only_v6: false,
                reuse_port: false,
                reuse_addr: false,
                send_buf_size: None,
                recv_buf_size: None,
                send_timeout: None,
                recv_timeout: None,
                connect_timeout: None,
                accept_timeout: None,
            }),
        },
        _ => return Errno::Notsup,
    };

    let inode = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind,
        false,
        "socket".to_string(),
    );
    let rights = Rights::all_socket();
    let fd = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    wasi_try_mem!(ro_sock.write(&memory, fd));

    Errno::Success
}

/// ### `sock_set_opt_flag()`
/// Sets a particular socket setting
/// Note: This is similar to `setsockopt` in POSIX for SO_REUSEADDR
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be set
/// * `flag` - Value to set the option to
pub fn sock_set_opt_flag(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    flag: Bool,
) -> Errno {
    debug!("wasi::sock_set_opt_flag(ty={})", opt);

    let flag = match flag {
        Bool::False => false,
        Bool::True => true,
        _ => return Errno::Inval,
    };

    let option: super::state::WasiSocketOption = opt.into();
    wasi_try!(__sock_actor_mut(&ctx, sock, Rights::empty(), |socket| {
        socket.set_opt_flag(option, flag)
    }));
    Errno::Success
}

/// ### `sock_get_opt_flag()`
/// Retrieve status of particular socket seting
/// Note: This is similar to `getsockopt` in POSIX for SO_REUSEADDR
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be retrieved
pub fn sock_get_opt_flag<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_flag: WasmPtr<Bool, M>,
) -> Errno {
    debug!("wasi::sock_get_opt_flag(ty={})", opt);
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let option: super::state::WasiSocketOption = opt.into();
    let flag = wasi_try!(__sock_actor(&ctx, sock, Rights::empty(), |socket| {
        socket.get_opt_flag(option)
    }));
    let flag = match flag {
        false => Bool::False,
        true => Bool::True,
    };

    wasi_try_mem!(ret_flag.write(&memory, flag));

    Errno::Success
}

/// ### `sock_set_opt_time()`
/// Sets one of the times the socket
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be set
/// * `time` - Value to set the time to
pub fn sock_set_opt_time<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    time: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    debug!("wasi::sock_set_opt_time(ty={})", opt);

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let time = wasi_try_mem!(time.read(&memory));
    let time = match time.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(time.u)),
        _ => return Errno::Inval,
    };

    let ty = match opt {
        Sockoption::RecvTimeout => wasmer_vnet::TimeType::ReadTimeout,
        Sockoption::SendTimeout => wasmer_vnet::TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => wasmer_vnet::TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => wasmer_vnet::TimeType::AcceptTimeout,
        Sockoption::Linger => wasmer_vnet::TimeType::Linger,
        _ => return Errno::Inval,
    };

    let option: super::state::WasiSocketOption = opt.into();
    wasi_try!(__sock_actor_mut(&ctx, sock, Rights::empty(), |socket| {
        socket.set_opt_time(ty, time)
    }));
    Errno::Success
}

/// ### `sock_get_opt_time()`
/// Retrieve one of the times on the socket
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be retrieved
pub fn sock_get_opt_time<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_time: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    debug!("wasi::sock_get_opt_time(ty={})", opt);
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let ty = match opt {
        Sockoption::RecvTimeout => wasmer_vnet::TimeType::ReadTimeout,
        Sockoption::SendTimeout => wasmer_vnet::TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => wasmer_vnet::TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => wasmer_vnet::TimeType::AcceptTimeout,
        Sockoption::Linger => wasmer_vnet::TimeType::Linger,
        _ => return Errno::Inval,
    };

    let time = wasi_try!(__sock_actor(&ctx, sock, Rights::empty(), |socket| {
        socket.opt_time(ty)
    }));
    let time = match time {
        None => OptionTimestamp {
            tag: OptionTag::None,
            u: 0,
        },
        Some(timeout) => OptionTimestamp {
            tag: OptionTag::Some,
            u: timeout.as_nanos() as Timestamp,
        },
    };

    wasi_try_mem!(ret_time.write(&memory, time));

    Errno::Success
}

/// ### `sock_set_opt_size()
/// Set size of particular option for this socket
/// Note: This is similar to `setsockopt` in POSIX for SO_RCVBUF
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `opt` - Socket option to be set
/// * `size` - Buffer size
pub fn sock_set_opt_size(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    size: Filesize,
) -> Errno {
    debug!("wasi::sock_set_opt_size(ty={})", opt);

    let ty = match opt {
        Sockoption::RecvTimeout => wasmer_vnet::TimeType::ReadTimeout,
        Sockoption::SendTimeout => wasmer_vnet::TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => wasmer_vnet::TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => wasmer_vnet::TimeType::AcceptTimeout,
        Sockoption::Linger => wasmer_vnet::TimeType::Linger,
        _ => return Errno::Inval,
    };

    let option: super::state::WasiSocketOption = opt.into();
    wasi_try!(__sock_actor_mut(&ctx, sock, Rights::empty(), |socket| {
        match opt {
            Sockoption::RecvBufSize => socket.set_recv_buf_size(size as usize),
            Sockoption::SendBufSize => socket.set_send_buf_size(size as usize),
            Sockoption::Ttl => socket.set_ttl(size as u32),
            Sockoption::MulticastTtlV4 => socket.set_multicast_ttl_v4(size as u32),
            _ => Err(Errno::Inval),
        }
    }));
    Errno::Success
}

/// ### `sock_get_opt_size()`
/// Retrieve the size of particular option for this socket
/// Note: This is similar to `getsockopt` in POSIX for SO_RCVBUF
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be retrieved
pub fn sock_get_opt_size<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_size: WasmPtr<Filesize, M>,
) -> Errno {
    debug!("wasi::sock_get_opt_size(ty={})", opt);
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let size = wasi_try!(__sock_actor(&ctx, sock, Rights::empty(), |socket| {
        match opt {
            Sockoption::RecvBufSize => socket.recv_buf_size().map(|a| a as Filesize),
            Sockoption::SendBufSize => socket.send_buf_size().map(|a| a as Filesize),
            Sockoption::Ttl => socket.ttl().map(|a| a as Filesize),
            Sockoption::MulticastTtlV4 => socket.multicast_ttl_v4().map(|a| a as Filesize),
            _ => Err(Errno::Inval),
        }
    }));
    wasi_try_mem!(ret_size.write(&memory, size));

    Errno::Success
}

/// ### `sock_join_multicast_v4()`
/// Joins a particular multicast IPv4 group
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `multiaddr` - Multicast group to joined
/// * `interface` - Interface that will join
pub fn sock_join_multicast_v4<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, M>,
    iface: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Errno {
    debug!("wasi::sock_join_multicast_v4");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v4(&memory, multiaddr));
    let iface = wasi_try!(super::state::read_ip_v4(&memory, iface));
    wasi_try!(__sock_actor_mut(&ctx, sock, Rights::empty(), |socket| {
        socket.join_multicast_v4(multiaddr, iface)
    }));
    Errno::Success
}

/// ### `sock_leave_multicast_v4()`
/// Leaves a particular multicast IPv4 group
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `multiaddr` - Multicast group to leave
/// * `interface` - Interface that will left
pub fn sock_leave_multicast_v4<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, M>,
    iface: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Errno {
    debug!("wasi::sock_leave_multicast_v4");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v4(&memory, multiaddr));
    let iface = wasi_try!(super::state::read_ip_v4(&memory, iface));
    wasi_try!(__sock_actor_mut(&ctx, sock, Rights::empty(), |socket| {
        socket.leave_multicast_v4(multiaddr, iface)
    }));
    Errno::Success
}

/// ### `sock_join_multicast_v6()`
/// Joins a particular multicast IPv6 group
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `multiaddr` - Multicast group to joined
/// * `interface` - Interface that will join
pub fn sock_join_multicast_v6<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, M>,
    iface: u32,
) -> Errno {
    debug!("wasi::sock_join_multicast_v6");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v6(&memory, multiaddr));
    wasi_try!(__sock_actor_mut(&ctx, sock, Rights::empty(), |socket| {
        socket.join_multicast_v6(multiaddr, iface)
    }));
    Errno::Success
}

/// ### `sock_leave_multicast_v6()`
/// Leaves a particular multicast IPv6 group
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `multiaddr` - Multicast group to leave
/// * `interface` - Interface that will left
pub fn sock_leave_multicast_v6<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, M>,
    iface: u32,
) -> Errno {
    debug!("wasi::sock_leave_multicast_v6");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v6(&memory, multiaddr));
    wasi_try!(__sock_actor_mut(&ctx, sock, Rights::empty(), |socket| {
        socket.leave_multicast_v6(multiaddr, iface)
    }));
    Errno::Success
}

/// ### `sock_bind()`
/// Bind a socket
/// Note: This is similar to `bind` in POSIX using PF_INET
///
/// ## Parameters
///
/// * `fd` - File descriptor of the socket to be bind
/// * `addr` - Address to bind the socket to
pub fn sock_bind<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!("wasi::sock_bind");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let addr = wasi_try!(super::state::read_ip_port(&memory, addr));
    let addr = SocketAddr::new(addr.0, addr.1);
    wasi_try!(__sock_upgrade(&ctx, sock, Rights::SOCK_BIND, |socket| {
        socket.bind(env.net(), addr)
    }));
    Errno::Success
}

/// ### `sock_listen()`
/// Listen for connections on a socket
///
/// Polling the socket handle will wait until a connection
/// attempt is made
///
/// Note: This is similar to `listen`
///
/// ## Parameters
///
/// * `fd` - File descriptor of the socket to be bind
/// * `backlog` - Maximum size of the queue for pending connections
pub fn sock_listen<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    backlog: M::Offset,
) -> Errno {
    debug!("wasi::sock_listen");

    let env = ctx.data();
    let backlog: usize = wasi_try!(backlog.try_into().map_err(|_| Errno::Inval));
    wasi_try!(__sock_upgrade(&ctx, sock, Rights::SOCK_BIND, |socket| {
        socket.listen(env.net(), backlog)
    }));
    Errno::Success
}

/// ### `sock_accept()`
/// Accept a new incoming connection.
/// Note: This is similar to `accept` in POSIX.
///
/// ## Parameters
///
/// * `fd` - The listening socket.
/// * `flags` - The desired values of the file descriptor flags.
///
/// ## Return
///
/// New socket connection
pub fn sock_accept<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    debug!("wasi::sock_accept");

    let env = ctx.data();
    let (child, addr) = {
        let mut ret;
        let (_, state) = env.get_memory_and_wasi_state(&ctx, 0);
        loop {
            wasi_try_ok!(
                match __sock_actor(&ctx, sock, Rights::SOCK_ACCEPT, |socket| socket
                    .accept_timeout(fd_flags, Duration::from_millis(5)))
                {
                    Ok(a) => {
                        ret = a;
                        break;
                    }
                    Err(Errno::Timedout) => {
                        env.yield_now()?;
                        continue;
                    }
                    Err(Errno::Again) => {
                        env.sleep(Duration::from_millis(5))?;
                        continue;
                    }
                    Err(err) => Err(err),
                }
            );
        }
        ret
    };

    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::TcpStream(child)),
    };
    let inode = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind,
        false,
        "socket".to_string(),
    );

    let rights = Rights::all_socket();
    let fd = wasi_try_ok!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    wasi_try_mem_ok!(ro_fd.write(&memory, fd));
    wasi_try_ok!(super::state::write_ip_port(
        &memory,
        ro_addr,
        addr.ip(),
        addr.port()
    ));

    Ok(Errno::Success)
}

/// ### `sock_connect()`
/// Initiate a connection on a socket to the specified address
///
/// Polling the socket handle will wait for data to arrive or for
/// the socket status to change which can be queried via 'sock_status'
///
/// Note: This is similar to `connect` in POSIX
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `addr` - Address of the socket to connect to
pub fn sock_connect<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!("wasi::sock_connect");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let addr = wasi_try!(super::state::read_ip_port(&memory, addr));
    let addr = SocketAddr::new(addr.0, addr.1);
    wasi_try!(__sock_upgrade(&ctx, sock, Rights::SOCK_CONNECT, |socket| {
        socket.connect(env.net(), addr)
    }));
    Errno::Success
}

/// ### `sock_recv()`
/// Receive a message from a socket.
/// Note: This is similar to `recv` in POSIX, though it also supports reading
/// the data into multiple buffers in the manner of `readv`.
///
/// ## Parameters
///
/// * `ri_data` - List of scatter/gather vectors to which to store data.
/// * `ri_flags` - Message flags.
///
/// ## Return
///
/// Number of bytes stored in ri_data and message flags.
pub fn sock_recv<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    _ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> Result<Errno, WasiError> {
    debug!("wasi::sock_recv");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));

    let bytes_read = wasi_try_ok!(__sock_actor_mut(&ctx, sock, Rights::SOCK_RECV, |socket| {
        socket.recv(&memory, iovs_arr)
    }));
    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));

    wasi_try_mem_ok!(ro_flags.write(&memory, 0));
    wasi_try_mem_ok!(ro_data_len.write(&memory, bytes_read));

    Ok(Errno::Success)
}

/// ### `sock_recv_from()`
/// Receive a message and its peer address from a socket.
/// Note: This is similar to `recvfrom` in POSIX, though it also supports reading
/// the data into multiple buffers in the manner of `readv`.
///
/// ## Parameters
///
/// * `ri_data` - List of scatter/gather vectors to which to store data.
/// * `ri_flags` - Message flags.
///
/// ## Return
///
/// Number of bytes stored in ri_data and message flags.
pub fn sock_recv_from<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    _ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    debug!("wasi::sock_recv_from");

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));

    let bytes_read = wasi_try_ok!(__sock_actor_mut(
        &ctx,
        sock,
        Rights::SOCK_RECV_FROM,
        |socket| { socket.recv_from(&memory, iovs_arr, ro_addr) }
    ));
    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));

    wasi_try_mem_ok!(ro_flags.write(&memory, 0));
    wasi_try_mem_ok!(ro_data_len.write(&memory, bytes_read));

    Ok(Errno::Success)
}

/// ### `sock_send()`
/// Send a message on a socket.
/// Note: This is similar to `send` in POSIX, though it also supports writing
/// the data from multiple buffers in the manner of `writev`.
///
/// ## Parameters
///
/// * `si_data` - List of scatter/gather vectors to which to retrieve data
/// * `si_flags` - Message flags.
///
/// ## Return
///
/// Number of bytes transmitted.
pub fn sock_send<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    _si_flags: SiFlags,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    debug!("wasi::sock_send");
    let env = ctx.data();

    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));

    let bytes_written = wasi_try_ok!(__sock_actor_mut(&ctx, sock, Rights::SOCK_SEND, |socket| {
        socket.send(&memory, iovs_arr)
    }));

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ret_data_len.write(&memory, bytes_written));

    Ok(Errno::Success)
}

/// ### `sock_send_to()`
/// Send a message on a socket to a specific address.
/// Note: This is similar to `sendto` in POSIX, though it also supports writing
/// the data from multiple buffers in the manner of `writev`.
///
/// ## Parameters
///
/// * `si_data` - List of scatter/gather vectors to which to retrieve data
/// * `si_flags` - Message flags.
/// * `addr` - Address of the socket to send message to
///
/// ## Return
///
/// Number of bytes transmitted.
pub fn sock_send_to<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    _si_flags: SiFlags,
    addr: WasmPtr<__wasi_addr_port_t, M>,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    debug!("wasi::sock_send_to");
    let env = ctx.data();

    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));

    let bytes_written = wasi_try_ok!(__sock_actor_mut(
        &ctx,
        sock,
        Rights::SOCK_SEND_TO,
        |socket| { socket.send_to::<M>(&memory, iovs_arr, addr) }
    ));

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ret_data_len.write(&memory, bytes_written as M::Offset));

    Ok(Errno::Success)
}

/// ### `sock_send_file()`
/// Sends the entire contents of a file down a socket
///
/// ## Parameters
///
/// * `in_fd` - Open file that has the data to be transmitted
/// * `offset` - Offset into the file to start reading at
/// * `count` - Number of bytes to be sent
///
/// ## Return
///
/// Number of bytes transmitted.
pub unsafe fn sock_send_file<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    in_fd: WasiFd,
    offset: Filesize,
    mut count: Filesize,
    ret_sent: WasmPtr<Filesize, M>,
) -> Result<Errno, WasiError> {
    debug!("wasi::send_file");
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    // Set the offset of the file
    {
        let mut fd_map = state.fs.fd_map.write().unwrap();
        let fd_entry = wasi_try_ok!(fd_map.get_mut(&in_fd).ok_or(Errno::Badf));
        fd_entry.offset = offset as u64;
    }

    // Enter a loop that will process all the data
    let mut total_written: Filesize = 0;
    while (count > 0) {
        let mut buf = [0; 4096];
        let sub_count = count.min(4096);
        count -= sub_count;

        let fd_entry = wasi_try_ok!(state.fs.get_fd(in_fd));
        let bytes_read = match in_fd {
            __WASI_STDIN_FILENO => {
                let mut guard = wasi_try_ok!(
                    inodes
                        .stdin_mut(&state.fs.fd_map)
                        .map_err(fs_error_into_wasi_err),
                    env
                );
                if let Some(ref mut stdin) = guard.deref_mut() {
                    wasi_try_ok!(stdin.read(&mut buf).map_err(map_io_err))
                } else {
                    return Ok(Errno::Badf);
                }
            }
            __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => return Ok(Errno::Inval),
            _ => {
                if !fd_entry.rights.contains(Rights::FD_READ) {
                    // TODO: figure out the error to return when lacking rights
                    return Ok(Errno::Access);
                }

                let offset = fd_entry.offset as usize;
                let inode_idx = fd_entry.inode;
                let inode = &inodes.arena[inode_idx];

                let bytes_read = {
                    let mut guard = inode.write();
                    let deref_mut = guard.deref_mut();
                    match deref_mut {
                        Kind::File { handle, .. } => {
                            if let Some(handle) = handle {
                                wasi_try_ok!(
                                    handle
                                        .seek(std::io::SeekFrom::Start(offset as u64))
                                        .map_err(map_io_err),
                                    env
                                );
                                wasi_try_ok!(handle.read(&mut buf).map_err(map_io_err))
                            } else {
                                return Ok(Errno::Inval);
                            }
                        }
                        Kind::Socket { socket } => {
                            wasi_try_ok!(socket.read(&mut buf).map_err(map_io_err))
                        }
                        Kind::Pipe { pipe } => {
                            wasi_try_ok!(pipe.read(&mut buf).map_err(map_io_err))
                        }
                        Kind::Dir { .. } | Kind::Root { .. } => {
                            return Ok(Errno::Isdir);
                        }
                        Kind::EventNotifications { .. } => {
                            return Ok(Errno::Inval);
                        }
                        Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_read"),
                        Kind::Buffer { buffer } => {
                            let mut buf_read = &buffer[offset..];
                            wasi_try_ok!(buf_read.read(&mut buf).map_err(map_io_err))
                        }
                    }
                };

                // reborrow
                let mut fd_map = state.fs.fd_map.write().unwrap();
                let fd_entry = wasi_try_ok!(fd_map.get_mut(&in_fd).ok_or(Errno::Badf));
                fd_entry.offset += bytes_read as u64;

                bytes_read
            }
        };

        // Write it down to the socket
        let bytes_written =
            wasi_try_ok!(__sock_actor_mut(&ctx, sock, Rights::SOCK_SEND, |socket| {
                let buf = (&buf[..]).to_vec();
                socket.send_bytes::<M>(Bytes::from(buf))
            }));
        total_written += bytes_written as u64;
    }

    wasi_try_mem_ok!(ret_sent.write(&memory, total_written as Filesize));

    Ok(Errno::Success)
}

/// ### `resolve()`
/// Resolves a hostname and a port to one or more IP addresses.
///
/// Note: This is similar to `getaddrinfo` in POSIX
///
/// When successful, the contents of the output buffer consist of a sequence of
/// IPv4 and/or IPv6 addresses. Each address entry consists of a addr_t object.
/// This function fills the output buffer as much as possible.
///
/// ## Parameters
///
/// * `host` - Host to resolve
/// * `port` - Port hint (zero if no hint is supplied)
/// * `addrs` - The buffer where addresses will be stored
///
/// ## Return
///
/// The number of IP addresses returned during the DNS resolution.
pub fn resolve<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    host: WasmPtr<u8, M>,
    host_len: M::Offset,
    port: u16,
    addrs: WasmPtr<__wasi_addr_t, M>,
    naddrs: M::Offset,
    ret_naddrs: WasmPtr<M::Offset, M>,
) -> Errno {
    debug!("wasi::resolve");

    let naddrs: usize = wasi_try!(naddrs.try_into().map_err(|_| Errno::Inval));
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let host_str = unsafe { get_input_str!(&memory, host, host_len) };
    let addrs = wasi_try_mem!(addrs.slice(&memory, wasi_try!(to_offset::<M>(naddrs))));

    let port = if port > 0 { Some(port) } else { None };

    let found_ips = wasi_try!(env
        .net()
        .resolve(host_str.as_str(), port, None)
        .map_err(net_error_into_wasi_err));

    let mut idx = 0;
    for found_ip in found_ips.iter().take(naddrs) {
        super::state::write_ip(&memory, addrs.index(idx).as_ptr::<M>(), *found_ip);
        idx += 1;
    }

    let idx: M::Offset = wasi_try!(idx.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(ret_naddrs.write(&memory, idx));

    Errno::Success
}
