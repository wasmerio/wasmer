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
#[cfg(any(target_family = "wasm"))]
pub mod wasm;
#[cfg(any(target_os = "windows"))]
pub mod windows;

pub mod legacy;

use self::types::{
    wasi::{
        Addressfamily, Advice, Bid, BusErrno, BusHandles, Cid, Clockid, Dircookie, Dirent, Errno,
        Event, EventFdReadwrite, Eventrwflags, Eventtype, ExitCode, Fd as WasiFd, Fdflags, Fdstat,
        Filesize, Filestat, Filetype, Fstflags, Linkcount, Longsize, OptionFd, Pid, Prestat,
        Rights, Snapshot0Clockid, Sockoption, Sockstatus, Socktype, StackSnapshot,
        StdioMode as WasiStdioMode, Streamsecurity, Subscription, SubscriptionFsReadwrite, Tid,
        Timestamp, TlKey, TlUser, TlVal, Tty, WasiHash, Whence, __wasi_busdataformat_t,
    },
    *,
};
#[cfg(feature = "os")]
use crate::bin_factory::spawn_exec_module;
use crate::state::{read_ip_port, write_ip_port, WasiProcessWait};
use crate::utils::map_io_err;
use crate::{
    current_caller_id, import_object_for_all_wasi_versions, VirtualTaskManager, WasiEnvInner,
    WasiFunctionEnv, WasiRuntimeImplementation, WasiVFork, DEFAULT_STACK_SIZE,
};
use crate::{
    mem_error_to_wasi,
    state::{
        self, bus_errno_into_vbus_error, fs_error_into_wasi_err, iterate_poll_events,
        net_error_into_wasi_err, poll, vbus_error_into_bus_errno,
        virtual_file_type_to_wasi_file_type, Inode, InodeHttpSocketType, InodeSocket,
        InodeSocketKind, InodeVal, Kind, PollEvent, PollEventBuilder, WasiBidirectionalPipePair,
        WasiBusCall, WasiDummyWaker, WasiFutex, WasiParkingLot, WasiProcessId, WasiState,
        WasiThreadContext, WasiThreadId, MAX_SYMLINKS,
    },
    Fd, WasiEnv, WasiError,
};
use crate::{runtime::SpawnType, WasiThread};
use bytes::{Bytes, BytesMut};
use cooked_waker::IntoWaker;
use sha2::Sha256;
use std::borrow::{Borrow, Cow};
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::convert::{Infallible, TryInto};
use std::io::{self, Read, Seek, Write};
use std::mem::transmute;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU64;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{atomic::Ordering, Mutex};
use std::sync::{mpsc, Arc, Condvar};
use std::task::{Context, Poll};
use std::thread::LocalKey;
use std::time::Duration;
use tracing::{debug, error, trace, warn};
use wasmer::vm::VMMemory;
use wasmer::{
    AsStoreMut, AsStoreRef, Extern, Function, FunctionEnv, FunctionEnvMut, Global, Instance,
    Memory, Memory32, Memory64, MemoryAccessError, MemoryError, MemorySize, MemoryView, Module,
    OnCalledAction, Pages, RuntimeError, Store, StoreSnapshot, TypedFunction, Value, WasmPtr,
    WasmSlice,
};
use wasmer_vbus::{
    BusDataFormat, BusInvocationEvent, BusSpawnedProcess, FileDescriptor, SignalHandlerAbi,
    SpawnOptionsConfig, StdioMode, VirtualBusError, VirtualBusInvokedWait,
};
use wasmer_vfs::{FileSystem, FsError, VirtualFile};
use wasmer_vnet::{SocketHttpRequest, StreamSecurity};
use wasmer_wasi_types::{asyncify::__wasi_asyncify_t, wasi::EventUnion};

#[cfg(any(
    target_os = "freebsd",
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple"
))]
pub use unix::*;

#[cfg(any(target_os = "windows"))]
pub use windows::*;

#[cfg(any(target_family = "wasm"))]
pub use wasm::*;

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

/// Writes data to the stderr
pub fn stderr_write(ctx: &FunctionEnvMut<'_, WasiEnv>, buf: &[u8]) -> Result<(), Errno> {
    let env = ctx.data();
    let (memory, state, inodes) = env.get_memory_and_wasi_state_and_inodes_mut(ctx, 0);

    let mut stderr = inodes
        .stderr_mut(&state.fs.fd_map)
        .map_err(fs_error_into_wasi_err)?;

    stderr.write_all(buf).map_err(map_io_err)
}

/// Performs an immuatble operation on the socket while running in an asynchronous runtime
/// This has built in signal support
fn __sock_actor<T, F, Fut>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<T, Errno>
where
    T: 'static,
    F: FnOnce(crate::state::InodeSocket) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<T, Errno>>,
{
    let env = ctx.data();
    let state = env.state.clone();
    let inodes = state.inodes.clone();

    let fd_entry = state.fs.get_fd(sock)?;
    let ret = {
        if !rights.is_empty() && !fd_entry.rights.contains(rights) {
            return Err(Errno::Access);
        }

        let inodes_guard = inodes.read().unwrap();
        let inode_idx = fd_entry.inode;
        let inode = &inodes_guard.arena[inode_idx];

        let tasks = env.tasks.clone();
        let mut guard = inode.read();
        match guard.deref() {
            Kind::Socket { socket } => {
                // Clone the socket and release the lock
                let socket = socket.clone();
                drop(guard);

                // Block on the work and process process
                __asyncify(ctx, None, async move { actor(socket).await })?
            }
            _ => {
                return Err(Errno::Notsock);
            }
        }
    };

    Ok(ret)
}

/// Asyncify takes the current thread and blocks on the async runtime associated with it
/// thus allowed for asynchronous operations to execute. It has built in functionality
/// to (optionally) timeout the IO, force exit the process, callback signals and pump
/// synchronous IO engine
fn __asyncify<T, Fut>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    timeout: Option<Duration>,
    work: Fut,
) -> Result<T, Errno>
where
    T: 'static,
    Fut: std::future::Future<Output = Result<T, Errno>> + 'static,
{
    let mut env = ctx.data();

    /*
    // Fast path (inline synchronous)
    {
        let _guard = env.tasks.enter();
        let waker = WasiDummyWaker.into_waker();
        let mut cx = Context::from_waker(&waker);
        let pinned_work = Box::pin(work);
        let mut pinned_work = pinned_work.as_mut();
        if let Poll::Ready(i) = pinned_work.poll(&mut cx) {
            return i;
        }
    }
    */

    // Slow path (will may put the thread to sleep)
    //let mut env = ctx.data();
    let tasks = env.tasks.clone();

    // Create the timeout
    let timeout = {
        let tasks_inner = tasks.clone();
        async move {
            if let Some(timeout) = timeout {
                tasks_inner
                    .sleep_now(current_caller_id(), timeout.as_millis())
                    .await
            } else {
                InfiniteSleep::default().await
            }
        }
    };

    let mut signaler = {
        let signals = env.thread.signals.lock().unwrap();
        signals.1.subscribe()
    };

    // Check if we need to exit the asynchronous loop
    if env.should_exit().is_some() {
        return Err(Errno::Intr);
    }

    // Block on the work and process process
    let tasks_inner = tasks.clone();
    let (tx_ret, mut rx_ret) = tokio::sync::mpsc::unbounded_channel();
    tasks.block_on(Box::pin(async move {
        tokio::select! {
            // The main work we are doing
            ret = work => {
                let _ = tx_ret.send(Some(ret));
            },
            // If a signaller is triggered then we interrupt the main process
            _ = signaler.recv() => {
                let _ = tx_ret.send(None);
            },
            // Optional timeout
            _ = timeout => {
                let _ = tx_ret.send(Some(Err(Errno::Timedout)));
            },
            // Periodically wake every 10 milliseconds for synchronously IO
            // (but only if someone is currently registered for it)
            _ = async move {
                loop {
                    tasks_inner.wait_for_root_waker().await;
                    tasks_inner.wake_root_wakers();
                }
            } => { }
        }
    }));

    // If a signal is received then we need to process it and if
    // we can not then fail with an interrupt error code
    let ret = rx_ret.try_recv().map_err(|_| Errno::Intr)?;
    return match ret {
        Some(a) => a,
        None => {
            ctx.data().clone().process_signals(ctx)?;
            Err(Errno::Intr)
        }
    };
}

// This should be compiled away, it will simply wait forever however its never
// used by itself, normally this is passed into asyncify which will still abort
// the operating on timeouts, signals or other work due to a select! around the await
#[derive(Default)]
struct InfiniteSleep {}
impl std::future::Future for InfiniteSleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

/// Performs mutable work on a socket under an asynchronous runtime with
/// built in signal processing
fn __sock_actor_mut<T, F, Fut>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<T, Errno>
where
    T: 'static,
    F: FnOnce(crate::state::InodeSocket) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<T, Errno>>,
{
    let env = ctx.data();
    let state = env.state.clone();
    let inodes = state.inodes.clone();

    let fd_entry = state.fs.get_fd(sock)?;
    if !rights.is_empty() && !fd_entry.rights.contains(rights) {
        return Err(Errno::Access);
    }

    let tasks = env.tasks.clone();
    {
        let inode_idx = fd_entry.inode;
        let inodes_guard = inodes.read().unwrap();
        let inode = &inodes_guard.arena[inode_idx];
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::Socket { socket } => {
                // Clone the socket and release the lock
                let socket = socket.clone();
                drop(guard);
                drop(inodes_guard);

                __asyncify(ctx, None, async move { actor(socket).await })
            }
            _ => {
                return Err(Errno::Notsock);
            }
        }
    }
}

/// Replaces a socket with another socket in under an asynchronous runtime.
/// This is used for opening sockets or connecting sockets which changes
/// the fundamental state of the socket to another state machine
fn __sock_upgrade<F, Fut>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<(), Errno>
where
    F: FnOnce(crate::state::InodeSocket) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<Option<crate::state::InodeSocket>, Errno>>,
{
    let env = ctx.data();
    let state = env.state.clone();
    let inodes = state.inodes.clone();

    let fd_entry = state.fs.get_fd(sock)?;
    if !rights.is_empty() && !fd_entry.rights.contains(rights) {
        tracing::warn!(
            "wasi[{}:{}]::sock_upgrade(fd={}, rights={:?}) - failed - no access rights to upgrade",
            ctx.data().pid(),
            ctx.data().tid(),
            sock,
            rights
        );
        return Err(Errno::Access);
    }

    let tasks = env.tasks.clone();
    {
        let inode_idx = fd_entry.inode;
        let inodes_guard = inodes.read().unwrap();
        let inode = &inodes_guard.arena[inode_idx];
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::Socket { socket } => {
                let socket = socket.clone();
                drop(guard);
                drop(inodes_guard);

                let new_socket = {
                    // Block on the work and process process
                    __asyncify(ctx, None, async move { actor(socket).await })?
                };

                if let Some(mut new_socket) = new_socket {
                    let inodes_guard = inodes.read().unwrap();
                    let inode = &inodes_guard.arena[inode_idx];
                    let mut guard = inode.write();
                    match guard.deref_mut() {
                        Kind::Socket { socket } => {
                            std::mem::swap(socket, &mut new_socket);
                        }
                        _ => {
                            tracing::warn!(
                                "wasi[{}:{}]::sock_upgrade(fd={}, rights={:?}) - failed - not a socket",
                                ctx.data().pid(),
                                ctx.data().tid(),
                                sock,
                                rights
                            );
                            return Err(Errno::Notsock);
                        }
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "wasi[{}:{}]::sock_upgrade(fd={}, rights={:?}) - failed - not a socket",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    sock,
                    rights
                );
                return Err(Errno::Notsock);
            }
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
    let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
    Ok(now as Timestamp)
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
    debug!("wasi[{}:{}]::args_get", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let args = state
        .args
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let result = write_buffer_array(&memory, &args, argv, argv_buf);

    debug!(
        "=> args:\n{}",
        state
            .args
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{:>20}: {}", i, v))
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
    debug!(
        "wasi[{}:{}]::args_sizes_get",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    clock_id: Clockid,
    resolution: WasmPtr<Timestamp, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::clock_res_get",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let out_addr = resolution.deref(&memory);
    let t_out = wasi_try!(platform_clock_res_get(
        Snapshot0Clockid::from(clock_id),
        out_addr
    ));
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
    clock_id: Clockid,
    precision: Timestamp,
    time: WasmPtr<Timestamp, M>,
) -> Errno {
    debug!(
        "wasi::clock_time_get clock_id: {}, precision: {}",
        clock_id as u8, precision
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let mut t_out = wasi_try!(platform_clock_time_get(
        Snapshot0Clockid::from(clock_id),
        precision
    ));
    {
        let guard = env.state.clock_offset.lock().unwrap();
        if let Some(offset) = guard.get(&clock_id) {
            t_out += *offset;
        }
    };
    wasi_try_mem!(time.write(&memory, t_out as Timestamp));

    let result = Errno::Success;
    /*
    trace!(
        "time: {} => {}",
        wasi_try_mem!(time.deref(&memory).read()),
        result
    );
    */
    result
}

/// ### `clock_time_set()`
/// Set the time of the specified clock
/// Inputs:
/// - `Clockid clock_id`
///     The ID of the clock to query
/// - `Timestamp *time`
///     The value of the clock in nanoseconds
pub fn clock_time_set<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    clock_id: Clockid,
    time: Timestamp,
) -> Errno {
    trace!(
        "wasi::clock_time_set clock_id: {:?}, time: {}",
        clock_id,
        time
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let snapshot_clock_id = match clock_id {
        Clockid::Realtime => Snapshot0Clockid::Realtime,
        Clockid::Monotonic => Snapshot0Clockid::Monotonic,
        Clockid::ProcessCputimeId => Snapshot0Clockid::ProcessCputimeId,
        Clockid::ThreadCputimeId => Snapshot0Clockid::ThreadCputimeId,
    };

    let precision = 1 as Timestamp;
    let t_now = wasi_try!(platform_clock_time_get(snapshot_clock_id, precision));
    let t_now = t_now as i64;

    let t_target = time as i64;
    let t_offset = t_target - t_now;

    let mut guard = env.state.clock_offset.lock().unwrap();
    guard.insert(clock_id, t_offset);

    Errno::Success
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

    write_buffer_array(&memory, &*state.envs, environ, environ_buf)
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
    trace!(
        "wasi[{}:{}]::environ_sizes_get",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::fd_advise: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );

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
    debug!(
        "wasi[{}:{}]::fd_allocate",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let mut handle = handle.write().unwrap();
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
    debug!(
        "wasi[{}:{}]::fd_close: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );
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
    debug!(
        "wasi[{}:{}]::fd_datasync",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
        "wasi[{}:{}]::fd_fdstat_get: fd={}, buf_ptr={}",
        ctx.data().pid(),
        ctx.data().tid(),
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
    debug!(
        "wasi[{}:{}]::fd_fdstat_set_flags (fd={}, flags={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        flags
    );
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
    let fd_entry = wasi_try!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
    let inode = fd_entry.inode;

    if !fd_entry.rights.contains(Rights::FD_FDSTAT_SET_FLAGS) {
        debug!(
            "wasi[{}:{}]::fd_fdstat_set_flags (fd={}, flags={:?}) - access denied",
            ctx.data().pid(),
            ctx.data().tid(),
            fd,
            flags
        );
        return Errno::Access;
    }

    {
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::Socket { socket } => {
                let nonblocking = flags.contains(Fdflags::NONBLOCK);
                debug!(
                    "wasi[{}:{}]::socket(fd={}) nonblocking={}",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    fd,
                    nonblocking
                );
                socket.set_nonblocking(nonblocking);
            }
            _ => {}
        }
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
    debug!(
        "wasi[{}:{}]::fd_fdstat_set_rights",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    fd_filestat_get_internal(&mut ctx, fd, buf)
}

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `__wasi_fd_t fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `__wasi_filestat_t *buf`
///     Where the metadata from `fd` will be written
pub(crate) fn fd_filestat_get_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_filestat_get",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::fd_filestat_set_size",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;

    if !fd_entry.rights.contains(Rights::FD_FILESTAT_SET_SIZE) {
        return Errno::Access;
    }

    {
        let mut guard = inodes.arena[inode].write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let mut handle = handle.write().unwrap();
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
    debug!(
        "wasi[{}:{}]::fd_filestat_set_times",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    ref_iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!(
        "wasi[{}:{}]::fd_pread: fd={}, offset={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        offset
    );
    let env = ctx.data();
    let (mut memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let mut iovs = wasi_try_mem_ok!(ref_iovs.slice(&memory, iovs_len));

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let is_non_blocking = fd_entry.flags.contains(Fdflags::NONBLOCK);

    let bytes_read = match fd {
        __WASI_STDIN_FILENO => {
            let mut stdin = wasi_try_ok!(inodes
                .stdin_mut(&state.fs.fd_map)
                .map_err(fs_error_into_wasi_err));
            wasi_try_ok!(read_bytes(stdin.deref_mut(), &memory, iovs))
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
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(h) = handle {
                        let mut h = h.write().unwrap();
                        wasi_try_ok!(h
                            .seek(std::io::SeekFrom::Start(offset as u64))
                            .map_err(map_io_err));
                        memory = env.memory_view(&ctx);
                        iovs = wasi_try_mem_ok!(ref_iovs.slice(&memory, iovs_len));
                        wasi_try_ok!(read_bytes(h.deref_mut(), &memory, iovs))
                    } else {
                        return Ok(Errno::Inval);
                    }
                }
                Kind::Socket { socket } => return Ok(Errno::Inval),
                Kind::Pipe { pipe } => return Ok(Errno::Inval),
                Kind::EventNotifications { .. } => return Ok(Errno::Inval),
                Kind::Dir { .. } | Kind::Root { .. } => return Ok(Errno::Isdir),
                Kind::Symlink { .. } => return Ok(Errno::Inval),
                Kind::Buffer { buffer } => {
                    wasi_try_ok!(read_bytes(&buffer[(offset as usize)..], &memory, iovs))
                }
            }
        }
    };

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));
    let nread_ref = nread.deref(&memory);
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
    trace!(
        "wasi[{}:{}]::fd_prestat_get: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );
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
        "wasi[{}:{}]::fd_prestat_dir_name: fd={}, path_len={}",
        ctx.data().pid(),
        ctx.data().tid(),
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
    match guard.deref() {
        Kind::Dir { .. } | Kind::Root { .. } => {
            // TODO: verify this: null termination, etc
            let path_len: u64 = path_len.into();
            if (inode_val.name.len() as u64) < path_len {
                wasi_try_mem!(path_chars
                    .subslice(0..inode_val.name.len() as u64)
                    .write_slice(inode_val.name.as_bytes()));
                wasi_try_mem!(path_chars.index(inode_val.name.len() as u64).write(0));

                //trace!("=> result: \"{}\"", inode_val.name);

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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!(
        "wasi[{}:{}]::fd_pwrite (fd={}, offset={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        offset,
    );
    // TODO: refactor, this is just copied from `fd_write`...
    let mut env = ctx.data();
    let state = env.state.clone();
    let inodes = state.inodes.clone();

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_written = {
        let inodes = inodes.read().unwrap();
        match fd {
            __WASI_STDIN_FILENO => return Ok(Errno::Inval),
            __WASI_STDOUT_FILENO => {
                let mut stdout = wasi_try_ok!(inodes
                    .stdout_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err));

                let memory = env.memory_view(&ctx);
                let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                wasi_try_ok!(write_bytes(stdout.deref_mut(), &memory, iovs_arr))
            }
            __WASI_STDERR_FILENO => {
                let mut stderr = wasi_try_ok!(inodes
                    .stderr_mut(&state.fs.fd_map)
                    .map_err(fs_error_into_wasi_err));

                let memory = env.memory_view(&ctx);
                let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                wasi_try_ok!(write_bytes(stderr.deref_mut(), &memory, iovs_arr))
            }
            _ => {
                if !fd_entry.rights.contains(Rights::FD_WRITE | Rights::FD_SEEK) {
                    return Ok(Errno::Access);
                }

                let inode_idx = fd_entry.inode;
                let inode = &inodes.arena[inode_idx];

                let mut guard = inode.write();
                match guard.deref_mut() {
                    Kind::File { handle, .. } => {
                        if let Some(handle) = handle {
                            let mut handle = handle.write().unwrap();
                            wasi_try_ok!(handle
                                .seek(std::io::SeekFrom::Start(offset as u64))
                                .map_err(map_io_err));
                            let memory = env.memory_view(&ctx);
                            let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                            wasi_try_ok!(write_bytes(handle.deref_mut(), &memory, iovs_arr))
                        } else {
                            return Ok(Errno::Inval);
                        }
                    }
                    Kind::Socket { socket } => {
                        let memory = env.memory_view(&ctx);
                        let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                        let buf_len: M::Offset = iovs_arr
                            .iter()
                            .filter_map(|a| a.read().ok())
                            .map(|a| a.buf_len)
                            .sum();
                        let buf_len: usize =
                            wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
                        let mut buf = Vec::with_capacity(buf_len);
                        wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));

                        let socket = socket.clone();
                        let ret = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                            socket.send(buf).await
                        }));
                        env = ctx.data();
                        ret
                    }
                    Kind::Pipe { pipe } => {
                        let memory = env.memory_view(&ctx);
                        let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                        wasi_try_ok!(pipe.send(&memory, iovs_arr))
                    }
                    Kind::Dir { .. } | Kind::Root { .. } => {
                        // TODO: verify
                        return Ok(Errno::Isdir);
                    }
                    Kind::EventNotifications { .. } => return Ok(Errno::Inval),
                    Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_pwrite"),
                    Kind::Buffer { buffer } => {
                        let memory = env.memory_view(&ctx);
                        let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                        wasi_try_ok!(write_bytes(
                            &mut buffer[(offset as usize)..],
                            &memory,
                            iovs_arr
                        ))
                    }
                }
            }
        }
    };

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    let memory = env.memory_view(&ctx);
    let nwritten_ref = nwritten.deref(&memory);
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!(
        "wasi[{}:{}]::fd_read: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );

    wasi_try_ok!(ctx.data().clone().process_signals(&mut ctx));

    let mut env = ctx.data();
    let state = env.state.clone();
    let inodes = state.inodes.clone();

    let is_stdio = match fd {
        __WASI_STDIN_FILENO => true,
        __WASI_STDOUT_FILENO => return Ok(Errno::Inval),
        __WASI_STDERR_FILENO => return Ok(Errno::Inval),
        _ => false,
    };

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let bytes_read = {
        if is_stdio == false {
            if !fd_entry.rights.contains(Rights::FD_READ) {
                // TODO: figure out the error to return when lacking rights
                return Ok(Errno::Access);
            }
        }

        let is_non_blocking = fd_entry.flags.contains(Fdflags::NONBLOCK);
        let offset = fd_entry.offset.load(Ordering::Acquire) as usize;
        let inode_idx = fd_entry.inode;

        let max_size = {
            let memory = env.memory_view(&ctx);
            let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
            let mut max_size = 0usize;
            for iovs in iovs_arr.iter() {
                let iovs = wasi_try_mem_ok!(iovs.read());
                let buf_len: usize =
                    wasi_try_ok!(iovs.buf_len.try_into().map_err(|_| Errno::Overflow));
                max_size += buf_len;
            }
            max_size
        };

        let bytes_read = {
            let inodes = inodes.read().unwrap();
            let inode = &inodes.arena[inode_idx];
            let mut guard = inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let handle = handle.clone();

                        let register_root_waker = env.tasks.register_root_waker();
                        let data = wasi_try_ok!(__asyncify(
                            &mut ctx,
                            if is_non_blocking {
                                Some(Duration::ZERO)
                            } else {
                                None
                            },
                            async move {
                                let mut handle = handle.write().unwrap();
                                if is_stdio == false {
                                    handle
                                        .seek(std::io::SeekFrom::Start(offset as u64))
                                        .map_err(map_io_err)?;
                                }

                                handle
                                    .read_async(max_size, &register_root_waker)
                                    .await
                                    .map_err(map_io_err)
                            }
                        )
                        .map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }));
                        env = ctx.data();

                        let memory = env.memory_view(&ctx);
                        let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                        wasi_try_ok!(read_bytes(&data[..], &memory, iovs_arr))
                    } else {
                        return Ok(Errno::Inval);
                    }
                }
                Kind::Socket { socket } => {
                    let socket = socket.clone();
                    let data = wasi_try_ok!(__asyncify(
                        &mut ctx,
                        if is_non_blocking {
                            Some(Duration::ZERO)
                        } else {
                            None
                        },
                        async move { socket.recv(max_size).await }
                    )
                    .map_err(|err| match err {
                        Errno::Timedout => Errno::Again,
                        a => a,
                    }));
                    env = ctx.data();

                    let data_len = data.len();
                    let mut reader = &data[..];
                    let memory = env.memory_view(&ctx);
                    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                    let bytes_read =
                        wasi_try_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| data_len));
                    bytes_read
                }
                Kind::Pipe { pipe } => {
                    let pipe = pipe.clone();
                    let data = wasi_try_ok!(__asyncify(
                        &mut ctx,
                        if is_non_blocking {
                            Some(Duration::ZERO)
                        } else {
                            None
                        },
                        async move { pipe.recv(max_size).await }
                    )
                    .map_err(|err| match err {
                        Errno::Timedout => Errno::Again,
                        a => a,
                    }));
                    env = ctx.data();

                    let data_len = data.len();
                    let mut reader = &data[..];

                    let memory = env.memory_view(&ctx);
                    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                    let bytes_read =
                        wasi_try_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| data_len));
                    bytes_read
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(Errno::Isdir);
                }
                Kind::EventNotifications {
                    counter: ref_counter,
                    is_semaphore: ref_is_semaphore,
                    wakers: ref_wakers,
                    ..
                } => {
                    let counter = Arc::clone(ref_counter);
                    let is_semaphore: bool = *ref_is_semaphore;
                    let wakers = Arc::clone(ref_wakers);

                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
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
                                .compare_exchange(val, new_val, Ordering::AcqRel, Ordering::Acquire)
                                .is_ok()
                            {
                                let mut memory = env.memory_view(&ctx);
                                let reader = val.to_ne_bytes();
                                let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                                ret = wasi_try_ok!(read_bytes(&reader[..], &memory, iovs_arr));
                                break;
                            } else {
                                continue;
                            }
                        }

                        // If its none blocking then exit
                        if is_non_blocking {
                            return Ok(Errno::Again);
                        }

                        // Yield until the notifications are triggered
                        let tasks_inner = env.tasks.clone();
                        rx = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                            let _ = rx.recv().await;
                            Ok(rx)
                        })
                        .map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }));
                        env = ctx.data();
                    }
                    ret
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_read"),
                Kind::Buffer { buffer } => {
                    let memory = env.memory_view(&ctx);
                    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));
                    wasi_try_ok!(read_bytes(&buffer[offset..], &memory, iovs_arr))
                }
            }
        };

        if is_stdio == false {
            // reborrow
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            fd_entry
                .offset
                .fetch_add(bytes_read as u64, Ordering::AcqRel);
        }

        bytes_read
    };

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));
    trace!(
        "wasi[{}:{}]::fd_read: bytes_read={}",
        ctx.data().pid(),
        ctx.data().tid(),
        bytes_read
    );

    let memory = env.memory_view(&ctx);
    let nread_ref = nread.deref(&memory);
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
    trace!(
        "wasi[{}:{}]::fd_readdir",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
        match guard.deref() {
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
    debug!(
        "wasi[{}:{}]::fd_renumber(from={}, to={})",
        ctx.data().pid(),
        ctx.data().tid(),
        from,
        to
    );
    if from == to {
        return Errno::Success;
    }
    let env = ctx.data();
    let (_, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&from).ok_or(Errno::Badf));

    fd_entry.ref_cnt.fetch_add(1, Ordering::Acquire);
    let new_fd_entry = Fd {
        // TODO: verify this is correct
        ref_cnt: fd_entry.ref_cnt.clone(),
        offset: fd_entry.offset.clone(),
        rights: fd_entry.rights_inheriting,
        ..*fd_entry
    };

    if let Some(fd_entry) = fd_map.get(&to).map(|a| a.clone()) {
        if fd_entry.ref_cnt.fetch_sub(1, Ordering::AcqRel) == 1 {
            wasi_try!(state.fs.close_fd_ext(inodes.deref(), &mut fd_map, to));
        }
    }
    fd_map.insert(to, new_fd_entry);

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
    debug!("wasi[{}:{}]::fd_dup", ctx.data().pid(), ctx.data().tid());

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
    debug!("wasi[{}:{}]::fd_event", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::EventNotifications {
        counter: Arc::new(AtomicU64::new(initial_val)),
        is_semaphore: flags & EVENT_FD_FLAGS_SEMAPHORE != 0,
        wakers: Default::default(),
        immediate: Arc::new(AtomicBool::new(false)),
    };

    let inode = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind,
        false,
        "event".to_string().into(),
    );
    let rights = Rights::FD_READ
        | Rights::FD_WRITE
        | Rights::POLL_FD_READWRITE
        | Rights::FD_FDSTAT_SET_FLAGS;
    let fd = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    debug!(
        "wasi[{}:{}]::fd_event - event notifications created (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );
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
    trace!(
        "wasi[{}:{}]::fd_seek: fd={}, offset={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        offset
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let new_offset_ref = newoffset.deref(&memory);
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_SEEK) {
        return Ok(Errno::Access);
    }

    // TODO: handle case if fd is a dir?
    let new_offset = match whence {
        Whence::Cur => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            if offset > 0 {
                fd_entry.offset.fetch_add(offset as u64, Ordering::AcqRel)
            } else if offset < 0 {
                fd_entry
                    .offset
                    .fetch_sub(offset.abs() as u64, Ordering::AcqRel)
            } else {
                fd_entry.offset.load(Ordering::Acquire)
            }
        }
        Whence::End => {
            use std::io::SeekFrom;
            let inode_idx = fd_entry.inode;
            let mut guard = inodes.arena[inode_idx].write();
            let deref_mut = guard.deref_mut();
            match deref_mut {
                Kind::File { ref mut handle, .. } => {
                    if let Some(handle) = handle {
                        let mut handle = handle.write().unwrap();
                        let end = wasi_try_ok!(handle.seek(SeekFrom::End(0)).map_err(map_io_err));

                        // TODO: handle case if fd_entry.offset uses 64 bits of a u64
                        drop(handle);
                        let mut fd_map = state.fs.fd_map.write().unwrap();
                        let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
                        fd_entry
                            .offset
                            .store((end as i64 + offset) as u64, Ordering::Release);
                        fd_entry
                            .offset
                            .store((end as i64 + offset) as u64, Ordering::Release);
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
            fd_entry.offset.load(Ordering::Acquire)
        }
        Whence::Set => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            fd_entry.offset.store(offset as u64, Ordering::Release);
            offset as u64
        }
        _ => return Ok(Errno::Inval),
    };
    // reborrow
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    wasi_try_mem_ok!(new_offset_ref.write(new_offset));

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
    debug!("wasi[{}:{}]::fd_sync", ctx.data().pid(), ctx.data().tid());
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
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(h) = handle {
                    let mut h = h.read().unwrap();
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
    debug!("wasi[{}:{}]::fd_tell", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let offset_ref = offset.deref(&memory);

    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_TELL) {
        return Errno::Access;
    }

    wasi_try_mem!(offset_ref.write(fd_entry.offset.load(Ordering::Acquire)));

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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    trace!(
        "wasi[{}:{}]::fd_write: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );
    let mut env = ctx.data();
    let state = env.state.clone();
    let mut memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));

    let is_stdio = match fd {
        __WASI_STDIN_FILENO => return Ok(Errno::Inval),
        __WASI_STDOUT_FILENO => true,
        __WASI_STDERR_FILENO => true,
        _ => false,
    };

    let bytes_written = {
        if is_stdio == false {
            if !fd_entry.rights.contains(Rights::FD_WRITE) {
                return Ok(Errno::Access);
            }
        }

        let is_non_blocking = fd_entry.flags.contains(Fdflags::NONBLOCK);
        let offset = fd_entry.offset.load(Ordering::Acquire) as usize;
        let inode_idx = fd_entry.inode;

        let bytes_written = {
            let (mut memory, _, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
            let inode = &inodes.arena[inode_idx];
            let mut guard = inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let buf_len: M::Offset = iovs_arr
                            .iter()
                            .filter_map(|a| a.read().ok())
                            .map(|a| a.buf_len)
                            .sum();
                        let buf_len: usize =
                            wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
                        let mut buf = Vec::with_capacity(buf_len);
                        wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));

                        let handle = handle.clone();
                        let register_root_waker = env.tasks.register_root_waker();
                        drop(inode);
                        drop(guard);
                        drop(inodes);
                        wasi_try_ok!(__asyncify(
                            &mut ctx,
                            if is_non_blocking {
                                Some(Duration::ZERO)
                            } else {
                                None
                            },
                            async move {
                                let mut handle = handle.write().unwrap();
                                if is_stdio == false {
                                    handle
                                        .seek(std::io::SeekFrom::Start(offset as u64))
                                        .map_err(map_io_err)?;
                                }

                                handle
                                    .write_async(&buf[..], &register_root_waker)
                                    .await
                                    .map_err(map_io_err)
                            }
                        )
                        .map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }))
                    } else {
                        return Ok(Errno::Inval);
                    }
                }
                Kind::Socket { socket } => {
                    let buf_len: M::Offset = iovs_arr
                        .iter()
                        .filter_map(|a| a.read().ok())
                        .map(|a| a.buf_len)
                        .sum();
                    let buf_len: usize = wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
                    let mut buf = Vec::with_capacity(buf_len);
                    wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));

                    let socket = socket.clone();
                    drop(guard);
                    drop(inodes);
                    wasi_try_ok!(__asyncify(
                        &mut ctx,
                        None,
                        async move { socket.send(buf).await }
                    ))
                }
                Kind::Pipe { pipe } => {
                    wasi_try_ok!(pipe.send(&memory, iovs_arr))
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(Errno::Isdir);
                }
                Kind::EventNotifications {
                    counter,
                    wakers,
                    immediate,
                    ..
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
                        immediate.store(true, Ordering::Release);
                        while let Some(wake) = guard.pop_back() {
                            let _ = wake.send(());
                        }
                    }

                    written
                }
                Kind::Symlink { .. } => return Ok(Errno::Inval),
                Kind::Buffer { buffer } => {
                    wasi_try_ok!(write_bytes(&mut buffer[offset..], &memory, iovs_arr))
                }
            }
        };
        env = ctx.data();
        memory = env.memory_view(&ctx);

        // reborrow and update the size
        if is_stdio == false {
            {
                let mut fd_map = state.fs.fd_map.write().unwrap();
                let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
                fd_entry
                    .offset
                    .fetch_add(bytes_written as u64, Ordering::AcqRel);
            }

            // we set the size but we don't return any errors if it fails as
            // pipes and sockets will not do anything with this
            let (mut memory, _, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
            let _ = state.fs.filestat_resync_size(inodes.deref(), fd);
        }
        bytes_written
    };

    let memory = env.memory_view(&ctx);
    let nwritten_ref = nwritten.deref(&memory);
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
    trace!("wasi[{}:{}]::fd_pipe", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let pipes = WasiBidirectionalPipePair::new();
    let pipe1 = pipes.send;
    let pipe2 = pipes.recv;

    let inode1 = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        Kind::Pipe { pipe: pipe1 },
        false,
        "pipe".to_string().into(),
    );
    let inode2 = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        Kind::Pipe { pipe: pipe2 },
        false,
        "pipe".to_string().into(),
    );

    let rights = Rights::FD_READ
        | Rights::FD_WRITE
        | Rights::FD_SYNC
        | Rights::FD_DATASYNC
        | Rights::POLL_FD_READWRITE
        | Rights::FD_FDSTAT_SET_FLAGS;
    let fd1 = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode1));
    let fd2 = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode2));
    trace!(
        "wasi[{}:{}]::fd_pipe (fd1={}, fd2={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd1,
        fd2
    );

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
    debug!(
        "wasi[{}:{}]::path_create_directory",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };
    debug!("=> fd: {}, path: {}", fd, &path_string);

    // Convert relative paths into absolute paths
    if path_string.starts_with("./") {
        path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_string
        );
    }

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
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };
    debug!(
        "wasi[{}:{}]::path_filestat_get (fd={}, path={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd,
        path_string
    );

    // Convert relative paths into absolute paths
    if path_string.starts_with("./") {
        path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_string
        );
    }

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
    debug!(
        "wasi[{}:{}]::path_filestat_set_times",
        ctx.data().pid(),
        ctx.data().tid()
    );
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

    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };
    debug!("=> base_fd: {}, path: {}", fd, &path_string);

    // Convert relative paths into absolute paths
    if path_string.starts_with("./") {
        path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_string
        );
    }

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
    debug!("wasi[{}:{}]::path_link", ctx.data().pid(), ctx.data().tid());
    if old_flags & __WASI_LOOKUP_SYMLINK_FOLLOW != 0 {
        debug!("  - will follow symlinks when opening path");
    }
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
    let mut old_path_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    let mut new_path_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
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

    // Convert relative paths into absolute paths
    old_path_str = ctx.data().state.fs.relative_path_to_absolute(old_path_str);
    new_path_str = ctx.data().state.fs.relative_path_to_absolute(new_path_str);

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
        match guard.deref_mut() {
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
    debug!("wasi[{}:{}]::path_open", ctx.data().pid(), ctx.data().tid());
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

    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };

    // Convert relative paths into absolute paths
    if path_string.starts_with("./") {
        path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_string
        );
    }
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
                *handle = Some(Arc::new(std::sync::RwLock::new(wasi_try!(open_options
                    .open(&path)
                    .map_err(fs_error_into_wasi_err)))));

                if let Some(handle) = handle {
                    let handle = handle.read().unwrap();
                    if let Some(special_fd) = handle.get_special_fd() {
                        // We close the file descriptor so that when its closed
                        // nothing bad happens
                        let special_fd = wasi_try!(state.fs.clone_fd(special_fd));

                        // some special files will return a constant FD rather than
                        // actually open the file (/dev/stdin, /dev/stdout, /dev/stderr)
                        wasi_try_mem!(fd_ref.write(special_fd));
                        return Errno::Success;
                    }
                }
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
                match guard.deref() {
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
                    handle: handle.map(|a| Arc::new(std::sync::RwLock::new(a))),
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
    debug!(
        "wasi[{}:{}]::path_open returning fd {}",
        ctx.data().pid(),
        ctx.data().tid(),
        out_fd
    );

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
    debug!(
        "wasi[{}:{}]::path_readlink",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(dir_fd));
    if !base_dir.rights.contains(Rights::PATH_READLINK) {
        return Errno::Access;
    }
    let mut path_str = unsafe { get_input_str!(&memory, path, path_len) };

    // Convert relative paths into absolute paths
    if path_str.starts_with("./") {
        path_str = ctx.data().state.fs.relative_path_to_absolute(path_str);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_str
        );
    }

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
    debug!(
        "wasi[{}:{}]::path_remove_directory",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    let mut path_str = unsafe { get_input_str!(&memory, path, path_len) };

    // Convert relative paths into absolute paths
    if path_str.starts_with("./") {
        path_str = ctx.data().state.fs.relative_path_to_absolute(path_str);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_str
        );
    }

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
        match guard.deref_mut() {
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
    let mut source_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    source_str = ctx.data().state.fs.relative_path_to_absolute(source_str);
    let source_path = std::path::Path::new(&source_str);
    let mut target_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
    target_str = ctx.data().state.fs.relative_path_to_absolute(target_str);
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
        match guard.deref() {
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
                error!("Fatal internal logic error: parent of inode is not a directory");
                return Errno::Inval;
            }
        }
    };

    let source_entry = {
        let mut guard = inodes.arena[source_parent_inode].write();
        match guard.deref_mut() {
            Kind::Dir { entries, .. } => {
                wasi_try!(entries.remove(&source_entry_name).ok_or(Errno::Noent))
            }
            Kind::Root { .. } => return Errno::Notcapable,
            Kind::Socket { .. } | Kind::Pipe { .. } | Kind::EventNotifications { .. } => {
                return Errno::Inval
            }
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                error!("Fatal internal logic error: parent of inode is not a directory");
                return Errno::Inval;
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
    debug!(
        "wasi[{}:{}]::path_symlink",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
    let mut old_path_str = unsafe { get_input_str!(&memory, old_path, old_path_len) };
    let mut new_path_str = unsafe { get_input_str!(&memory, new_path, new_path_len) };
    old_path_str = ctx.data().state.fs.relative_path_to_absolute(old_path_str);
    new_path_str = ctx.data().state.fs.relative_path_to_absolute(new_path_str);
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
        match guard.deref() {
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
        entry_name.clone().into(),
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
    debug!(
        "wasi[{}:{}]::path_unlink_file",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let base_dir = wasi_try!(state.fs.get_fd(fd));
    if !base_dir.rights.contains(Rights::PATH_UNLINK_FILE) {
        return Errno::Access;
    }
    let mut path_str = unsafe { get_input_str!(&memory, path, path_len) };
    debug!("Requested file: {}", path_str);

    // Convert relative paths into absolute paths
    if path_str.starts_with("./") {
        path_str = ctx.data().state.fs.relative_path_to_absolute(path_str);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            path_str
        );
    }

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
            let mut guard = inodes.arena[removed_inode].read();
            match guard.deref() {
                Kind::File { handle, path, .. } => {
                    if let Some(h) = handle {
                        let mut h = h.write().unwrap();
                        wasi_try!(h.unlink().map_err(fs_error_into_wasi_err));
                    } else {
                        // File is closed
                        // problem with the abstraction, we can't call unlink because there's no handle
                        // drop mutable borrow on `path`
                        let path = path.clone();
                        drop(guard);
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
/// - `__wasi_event_t *out`
///     The events that have occured
/// - `u32 nsubscriptions`
///     The number of subscriptions and the number of events
/// Output:
/// - `u32 nevents`
///     The number of events seen
pub fn poll_oneoff<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    in_: WasmPtr<Subscription, M>,
    out_: WasmPtr<Event, M>,
    nsubscriptions: M::Offset,
    nevents: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let mut env = ctx.data();
    let mut memory = env.memory_view(&ctx);

    let mut subscriptions = Vec::new();
    let subscription_array = wasi_try_mem_ok!(in_.slice(&memory, nsubscriptions));
    for sub in subscription_array.iter() {
        let s = wasi_try_mem_ok!(sub.read());
        subscriptions.push(s);
    }

    // Poll and receive all the events that triggered
    let triggered_events = poll_oneoff_internal(&mut ctx, subscriptions);
    let triggered_events = match triggered_events {
        Ok(a) => a,
        Err(err) => {
            tracing::trace!(
                "wasi[{}:{}]::poll_oneoff errno={}",
                ctx.data().pid(),
                ctx.data().tid(),
                err
            );
            return Ok(err);
        }
    };

    // Process all the events that were triggered
    let mut env = ctx.data();
    let mut memory = env.memory_view(&ctx);
    let mut events_seen: u32 = 0;
    let event_array = wasi_try_mem_ok!(out_.slice(&memory, nsubscriptions));
    for event in triggered_events {
        wasi_try_mem_ok!(event_array.index(events_seen as u64).write(event));
        events_seen += 1;
    }
    let events_seen: M::Offset = wasi_try_ok!(events_seen.try_into().map_err(|_| Errno::Overflow));
    let out_ptr = nevents.deref(&memory);
    wasi_try_mem_ok!(out_ptr.write(events_seen));
    Ok(Errno::Success)
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
pub(crate) fn poll_oneoff_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    subs: Vec<Subscription>,
) -> Result<Vec<Event>, Errno> {
    let pid = ctx.data().pid();
    let tid = ctx.data().tid();
    trace!(
        "wasi[{}:{}]::poll_oneoff (nsubscriptions={})",
        pid,
        tid,
        subs.len(),
    );

    // These are used when we capture what clocks (timeouts) are being
    // subscribed too
    let mut clock_subs = vec![];
    let mut time_to_sleep = None;

    // First we extract all the subscriptions into an array so that they
    // can be processed
    let mut env = ctx.data();
    let state = ctx.data().state.deref();
    let mut memory = env.memory_view(&ctx);
    let mut subscriptions = HashMap::new();
    for s in subs {
        let mut peb = PollEventBuilder::new();
        let mut in_events = HashMap::new();
        let fd = match s.type_ {
            Eventtype::FdRead => {
                let file_descriptor = unsafe { s.data.fd_readwrite.file_descriptor };
                match file_descriptor {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    fd => {
                        let fd_entry = state.fs.get_fd(fd)?;
                        if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                            return Err(Errno::Access);
                        }
                    }
                }
                in_events.insert(peb.add(PollEvent::PollIn).build(), s);
                file_descriptor
            }
            Eventtype::FdWrite => {
                let file_descriptor = unsafe { s.data.fd_readwrite.file_descriptor };
                match file_descriptor {
                    __WASI_STDIN_FILENO | __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => (),
                    fd => {
                        let fd_entry = state.fs.get_fd(fd)?;
                        if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                            return Err(Errno::Access);
                        }
                    }
                }
                in_events.insert(peb.add(PollEvent::PollOut).build(), s);
                file_descriptor
            }
            Eventtype::Clock => {
                let clock_info = unsafe { s.data.clock };
                if clock_info.clock_id == Clockid::Realtime
                    || clock_info.clock_id == Clockid::Monotonic
                {
                    // this is a hack
                    // TODO: do this properly
                    time_to_sleep = Some(Duration::from_nanos(clock_info.timeout));
                    clock_subs.push((clock_info, s.userdata));
                    continue;
                } else {
                    error!("Polling not implemented for these clocks yet");
                    return Err(Errno::Inval);
                }
            }
        };

        let entry = subscriptions
            .entry(fd)
            .or_insert_with(|| HashMap::<state::PollEventSet, Subscription>::default());
        entry.extend(in_events.into_iter());
    }
    drop(env);

    // If there is a timeout we need to use the runtime to measure this
    // otherwise we just process all the events and wait on them indefinately
    if let Some(time_to_sleep) = time_to_sleep.as_ref() {
        tracing::trace!(
            "wasi[{}:{}]::poll_oneoff wait_for_timeout={}",
            pid,
            tid,
            time_to_sleep.as_millis()
        );
    }
    let time_to_sleep = time_to_sleep;

    let mut events_seen: u32 = 0;

    // Build the async function we will block on
    let state = ctx.data().state.clone();
    let (triggered_events_tx, mut triggered_events_rx) = std::sync::mpsc::channel();
    let tasks = ctx.data().tasks.clone();
    let work = {
        let tasks = tasks.clone();
        let triggered_events_tx = triggered_events_tx.clone();
        async move {
            // We start by building a list of files we are going to poll
            // and open a read lock on them all
            let inodes = state.inodes.clone();
            let inodes = inodes.read().unwrap();
            let mut fd_guards = vec![];

            #[allow(clippy::significant_drop_in_scrutinee)]
            let fds = {
                for (fd, in_events) in subscriptions {
                    let wasi_file_ref = match fd {
                        __WASI_STDERR_FILENO => {
                            wasi_try_ok!(inodes
                                .stderr(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, in_events, tasks.clone()))
                                .map_err(fs_error_into_wasi_err))
                        }
                        __WASI_STDIN_FILENO => {
                            wasi_try_ok!(inodes
                                .stdin(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, in_events, tasks.clone()))
                                .map_err(fs_error_into_wasi_err))
                        }
                        __WASI_STDOUT_FILENO => {
                            wasi_try_ok!(inodes
                                .stdout(&state.fs.fd_map)
                                .map(|g| g.into_poll_guard(fd, in_events, tasks.clone()))
                                .map_err(fs_error_into_wasi_err))
                        }
                        _ => {
                            let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
                            if !fd_entry.rights.contains(Rights::POLL_FD_READWRITE) {
                                return Ok(Errno::Access);
                            }
                            let inode = fd_entry.inode;

                            {
                                let guard = inodes.arena[inode].read();
                                if let Some(guard) = crate::state::InodeValFilePollGuard::new(
                                    fd,
                                    guard.deref(),
                                    in_events,
                                    tasks.clone(),
                                ) {
                                    guard
                                } else {
                                    return Ok(Errno::Badf);
                                }
                            }
                        }
                    };
                    tracing::trace!(
                        "wasi[{}:{}]::poll_oneoff wait_for_fd={} type={:?}",
                        pid,
                        tid,
                        fd,
                        wasi_file_ref
                    );
                    fd_guards.push(wasi_file_ref);
                }

                fd_guards
            };

            // Build all the async calls we need for all the files
            let mut polls = Vec::new();
            for guard in fds {
                // Combine all the events together
                let mut peb = PollEventBuilder::new();
                for (in_events, _) in guard.subscriptions.iter() {
                    for in_event in iterate_poll_events(*in_events) {
                        peb = peb.add(in_event);
                    }
                }
                let peb = peb.build();

                let triggered_events_tx = triggered_events_tx.clone();
                let poll = Box::pin(async move {
                    let mut flags = 0;
                    let mut bytes_available = 0;

                    // Wait for it to trigger (or throw an error) then
                    // once it has triggered an event will be returned
                    // that we can give to the caller
                    let evts = guard.wait().await;
                    for evt in evts {
                        tracing::trace!(
                            "wasi[{}:{}]::poll_oneoff (fd_triggered={}, event={:?})",
                            pid,
                            tid,
                            guard.fd,
                            evt
                        );
                        triggered_events_tx.send(evt).unwrap();
                    }
                });
                polls.push(poll);
            }

            // We have to drop the lock on inodes otherwise it will freeze up the
            // IO subsystem
            drop(inodes);

            // This is the part that actually does the waiting
            if polls.is_empty() == false {
                futures::future::select_all(polls.into_iter()).await;
            } else {
                InfiniteSleep::default().await;
            }
            Ok(Errno::Success)
        }
    };

    // Block on the work and process process
    let mut env = ctx.data();
    let mut ret = __asyncify(ctx, time_to_sleep, async move { work.await });
    env = ctx.data();
    memory = env.memory_view(&ctx);

    // If its a timeout then return an event for it
    if let Err(Errno::Timedout) = ret {
        // The timeout has triggerred so lets add that event
        if clock_subs.len() <= 0 {
            tracing::warn!(
                "wasi[{}:{}]::poll_oneoff triggered_timeout (without any clock subscriptions)",
                pid,
                tid
            );
        }
        for (clock_info, userdata) in clock_subs {
            let evt = Event {
                userdata,
                error: Errno::Success,
                type_: Eventtype::Clock,
                u: EventUnion { clock: 0 },
            };
            tracing::trace!(
                "wasi[{}:{}]::poll_oneoff triggered_timeout (event={:?})",
                pid,
                tid,
                evt
            );
            triggered_events_tx.send(evt).unwrap();
        }
        ret = Ok(Errno::Success);
    }
    let ret = ret?;
    if ret != Errno::Success {
        return Err(ret);
    }

    // Process all the events that were triggered
    let mut event_array = Vec::new();
    while let Ok(event) = triggered_events_rx.try_recv() {
        event_array.push(event);
    }
    tracing::trace!(
        "wasi[{}:{}]::poll_oneoff seen={}",
        ctx.data().pid(),
        ctx.data().tid(),
        event_array.len()
    );
    Ok(event_array)
}

/// ### `proc_exit()`
/// Terminate the process normally. An exit code of 0 indicates successful
/// termination of the program. The meanings of other values is dependent on
/// the environment.
/// Inputs:
/// - `ExitCode`
///   Exit code to return to the operating system
pub fn proc_exit<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    code: ExitCode,
) -> Result<(), WasiError> {
    debug!(
        "wasi[{}:{}]::proc_exit (code={})",
        ctx.data().pid(),
        ctx.data().tid(),
        code
    );

    // Set the exit code for this process
    ctx.data().thread.terminate(code as u32);

    // If we are in a vfork we need to return to the point we left off
    #[cfg(feature = "os")]
    if let Some(mut vfork) = ctx.data_mut().vfork.take() {
        // Restore the WasiEnv to the point when we vforked
        std::mem::swap(&mut vfork.env.inner, &mut ctx.data_mut().inner);
        std::mem::swap(vfork.env.as_mut(), ctx.data_mut());
        let mut wasi_env = *vfork.env;
        wasi_env.owned_handles.push(vfork.handle);

        // We still need to create the process that exited so that
        // the exit code can be used by the parent process
        let pid = wasi_env.process.pid();
        let mut memory_stack = vfork.memory_stack;
        let rewind_stack = vfork.rewind_stack;
        let store_data = vfork.store_data;

        // If the return value offset is within the memory stack then we need
        // to update it here rather than in the real memory
        let pid_offset: u64 = vfork.pid_offset.into();
        if pid_offset >= wasi_env.stack_start && pid_offset < wasi_env.stack_base {
            // Make sure its within the "active" part of the memory stack
            let offset = wasi_env.stack_base - pid_offset;
            if offset as usize > memory_stack.len() {
                warn!("wasi[{}:{}]::vfork failed - the return value (pid) is outside of the active part of the memory stack ({} vs {})", ctx.data().pid(), ctx.data().tid(), offset, memory_stack.len());
                return Err(WasiError::Exit(Errno::Fault as u32));
            }

            // Update the memory stack with the new PID
            let val_bytes = pid.raw().to_ne_bytes();
            let pstart = memory_stack.len() - offset as usize;
            let pend = pstart + val_bytes.len();
            let pbytes = &mut memory_stack[pstart..pend];
            pbytes.clone_from_slice(&val_bytes);
        } else {
            warn!("wasi[{}:{}]::vfork failed - the return value (pid) is not being returned on the stack - which is not supported", ctx.data().pid(), ctx.data().tid());
            return Err(WasiError::Exit(Errno::Fault as u32));
        }

        // Jump back to the vfork point and current on execution
        unwind::<M, _>(ctx, move |mut ctx, _, _| {
            // Now rewind the previous stack and carry on from where we did the vfork
            match rewind::<M>(
                ctx,
                memory_stack.freeze(),
                rewind_stack.freeze(),
                store_data,
            ) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!("fork failed - could not rewind the stack - errno={}", err);
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
                }
            }
        })?;
        return Ok(());
    }

    // Otherwise just exit
    Err(WasiError::Exit(code))
}

/// ### `thread_signal()`
/// Send a signal to a particular thread in the current process.
/// Note: This is similar to `signal` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
#[cfg(feature = "os")]
pub fn thread_signal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    tid: Tid,
    sig: Signal,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::thread_signal(tid={}, sig={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        tid,
        sig
    );
    {
        let tid: WasiThreadId = tid.into();
        ctx.data().process.signal_thread(&tid, sig);
    }

    let env = ctx.data();

    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    Ok(Errno::Success)
}

#[cfg(not(feature = "os"))]
pub fn thread_signal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    tid: Tid,
    sig: Signal,
) -> Result<Errno, WasiError> {
    warn!(
        "wasi[{}:{}]::thread_signal(tid={}, sig={:?}) are not supported without the 'os' feature",
        ctx.data().pid(),
        ctx.data().tid(),
        tid,
        sig
    );
    Ok(Errno::Notsup)
}

/// ### `proc_raise()`
/// Send a signal to the process of the calling thread.
/// Note: This is similar to `raise` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
pub fn proc_raise(mut ctx: FunctionEnvMut<'_, WasiEnv>, sig: Signal) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::proc_raise (sig={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        sig
    );
    let env = ctx.data();
    env.process.signal_process(sig);

    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    Ok(Errno::Success)
}

/// ### `proc_raise()`
/// Send a signal to the process of the calling thread.
/// Note: This is similar to `raise` in POSIX.
/// Inputs:
/// - `Signal`
///   Signal to be raised for this process
pub fn proc_raise_interval(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sig: Signal,
    interval: Timestamp,
    repeat: Bool,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::proc_raise_interval (sig={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        sig
    );
    let env = ctx.data();
    let interval = match interval {
        0 => None,
        a => Some(Duration::from_millis(a)),
    };
    let repeat = match repeat {
        Bool::True => true,
        _ => false,
    };
    env.process.signal_interval(sig, interval, repeat);

    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    Ok(Errno::Success)
}

/// ### `sched_yield()`
/// Yields execution of the thread
pub fn sched_yield(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    //trace!("wasi[{}:{}]::sched_yield", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let tasks = env.tasks.clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        tasks.sleep_now(current_caller_id(), 0).await;
        Ok(())
    }));
    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);
    Ok(Errno::Success)
}

fn get_stack_base(mut ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> u64 {
    ctx.data().stack_base
}

fn get_stack_start(mut ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> u64 {
    ctx.data().stack_start
}

#[cfg(feature = "os")]
fn get_memory_stack_pointer(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> Result<u64, String> {
    // Get the current value of the stack pointer (which we will use
    // to save all of the stack)
    let stack_base = get_stack_base(ctx);
    let stack_pointer = if let Some(stack_pointer) = ctx.data().inner().stack_pointer.clone() {
        match stack_pointer.get(ctx) {
            Value::I32(a) => a as u64,
            Value::I64(a) => a as u64,
            _ => stack_base,
        }
    } else {
        return Err(format!(
            "failed to save stack: not exported __stack_pointer global"
        ));
    };
    Ok(stack_pointer)
}

#[cfg(feature = "os")]
fn get_memory_stack_offset(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> Result<u64, String> {
    let stack_base = get_stack_base(ctx);
    let stack_pointer = get_memory_stack_pointer(ctx)?;
    Ok(stack_base - stack_pointer)
}

#[cfg(feature = "os")]
fn set_memory_stack_offset(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    offset: u64,
) -> Result<(), String> {
    // Sets the stack pointer
    let stack_base = get_stack_base(ctx);
    let stack_pointer = stack_base - offset;
    if let Some(stack_pointer_ptr) = ctx.data().inner().stack_pointer.clone() {
        match stack_pointer_ptr.get(ctx) {
            Value::I32(_) => {
                stack_pointer_ptr.set(ctx, Value::I32(stack_pointer as i32));
            }
            Value::I64(_) => {
                stack_pointer_ptr.set(ctx, Value::I64(stack_pointer as i64));
            }
            _ => {
                return Err(format!(
                    "failed to save stack: __stack_pointer global is of an unknown type"
                ));
            }
        }
    } else {
        return Err(format!(
            "failed to save stack: not exported __stack_pointer global"
        ));
    }
    Ok(())
}

#[cfg(feature = "os")]
#[allow(dead_code)]
fn get_memory_stack<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
) -> Result<BytesMut, String> {
    // Get the current value of the stack pointer (which we will use
    // to save all of the stack)
    let stack_base = get_stack_base(ctx);
    let stack_pointer = if let Some(stack_pointer) = ctx.data().inner().stack_pointer.clone() {
        match stack_pointer.get(ctx) {
            Value::I32(a) => a as u64,
            Value::I64(a) => a as u64,
            _ => stack_base,
        }
    } else {
        return Err(format!(
            "failed to save stack: not exported __stack_pointer global"
        ));
    };
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let stack_offset = env.stack_base - stack_pointer;

    // Read the memory stack into a vector
    let memory_stack_ptr = WasmPtr::<u8, M>::new(
        stack_pointer
            .try_into()
            .map_err(|_| format!("failed to save stack: stack pointer overflow"))?,
    );

    memory_stack_ptr
        .slice(
            &memory,
            stack_offset
                .try_into()
                .map_err(|_| format!("failed to save stack: stack pointer overflow"))?,
        )
        .and_then(|memory_stack| memory_stack.read_to_bytes())
        .map_err(|err| format!("failed to read stack: {}", err))
}

#[allow(dead_code)]
#[cfg(feature = "os")]
fn set_memory_stack<M: MemorySize>(
    mut ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    stack: Bytes,
) -> Result<(), String> {
    // First we restore the memory stack
    let stack_base = get_stack_base(ctx);
    let stack_offset = stack.len() as u64;
    let stack_pointer = stack_base - stack_offset;
    let stack_ptr = WasmPtr::<u8, M>::new(
        stack_pointer
            .try_into()
            .map_err(|_| format!("failed to restore stack: stack pointer overflow"))?,
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    stack_ptr
        .slice(
            &memory,
            stack_offset
                .try_into()
                .map_err(|_| format!("failed to restore stack: stack pointer overflow"))?,
        )
        .and_then(|memory_stack| memory_stack.write_slice(&stack[..]))
        .map_err(|err| format!("failed to write stack: {}", err))?;

    // Set the stack pointer itself and return
    set_memory_stack_offset(ctx, stack_offset)?;
    Ok(())
}

#[cfg(feature = "os")]
#[must_use = "you must return the result immediately so the stack can unwind"]
fn unwind<M: MemorySize, F>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    callback: F,
) -> Result<Errno, WasiError>
where
    F: FnOnce(FunctionEnvMut<'_, WasiEnv>, BytesMut, BytesMut) -> OnCalledAction
        + Send
        + Sync
        + 'static,
{
    // Get the current stack pointer (this will be used to determine the
    // upper limit of stack space remaining to unwind into)
    let memory_stack = match get_memory_stack::<M>(&mut ctx) {
        Ok(a) => a,
        Err(err) => {
            warn!("unable to get the memory stack - {}", err);
            return Err(WasiError::Exit(Errno::Fault as ExitCode));
        }
    };

    // Perform a check to see if we have enough room
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    // Write the addresses to the start of the stack space
    let unwind_pointer: u64 = wasi_try_ok!(env.stack_start.try_into().map_err(|_| Errno::Overflow));
    let unwind_data_start =
        unwind_pointer + (std::mem::size_of::<__wasi_asyncify_t<M::Offset>>() as u64);
    let unwind_data = __wasi_asyncify_t::<M::Offset> {
        start: wasi_try_ok!(unwind_data_start.try_into().map_err(|_| Errno::Overflow)),
        end: wasi_try_ok!(env.stack_base.try_into().map_err(|_| Errno::Overflow)),
    };
    let unwind_data_ptr: WasmPtr<__wasi_asyncify_t<M::Offset>, M> =
        WasmPtr::new(wasi_try_ok!(unwind_pointer
            .try_into()
            .map_err(|_| Errno::Overflow)));
    wasi_try_mem_ok!(unwind_data_ptr.write(&memory, unwind_data));

    // Invoke the callback that will prepare to unwind
    // We need to start unwinding the stack
    let asyncify_data = wasi_try_ok!(unwind_pointer.try_into().map_err(|_| Errno::Overflow));
    if let Some(asyncify_start_unwind) = env.inner().asyncify_start_unwind.clone() {
        asyncify_start_unwind.call(&mut ctx, asyncify_data);
    } else {
        warn!("failed to unwind the stack because the asyncify_start_rewind export is missing");
        return Err(WasiError::Exit(128));
    }

    // Set callback that will be invoked when this process finishes
    let env = ctx.data();
    let unwind_stack_begin: u64 = unwind_data.start.into();
    let unwind_space = env.stack_base - env.stack_start;
    let func = ctx.as_ref();
    trace!(
        "wasi[{}:{}]::unwinding (memory_stack_size={} unwind_space={})",
        ctx.data().pid(),
        ctx.data().tid(),
        memory_stack.len(),
        unwind_space
    );
    ctx.as_store_mut().on_called(move |mut store| {
        let mut ctx = func.into_mut(&mut store);
        let env = ctx.data();
        let memory = env.memory_view(&ctx);

        let unwind_data_ptr: WasmPtr<__wasi_asyncify_t<M::Offset>, M> = WasmPtr::new(
            unwind_pointer
                .try_into()
                .map_err(|_| Errno::Overflow)
                .unwrap(),
        );
        let unwind_data_result = unwind_data_ptr.read(&memory).unwrap();
        let unwind_stack_finish: u64 = unwind_data_result.start.into();
        let unwind_size = unwind_stack_finish - unwind_stack_begin;
        trace!(
            "wasi[{}:{}]::unwound (memory_stack_size={} unwind_size={})",
            ctx.data().pid(),
            ctx.data().tid(),
            memory_stack.len(),
            unwind_size
        );

        // Read the memory stack into a vector
        let unwind_stack_ptr = WasmPtr::<u8, M>::new(
            unwind_stack_begin
                .try_into()
                .map_err(|_| format!("failed to save stack: stack pointer overflow"))?,
        );
        let unwind_stack = unwind_stack_ptr
            .slice(
                &memory,
                unwind_size
                    .try_into()
                    .map_err(|_| format!("failed to save stack: stack pointer overflow"))?,
            )
            .and_then(|memory_stack| memory_stack.read_to_bytes())
            .map_err(|err| format!("failed to read stack: {}", err))?;

        // Notify asyncify that we are no longer unwinding
        if let Some(asyncify_stop_unwind) = env.inner().asyncify_stop_unwind.clone() {
            asyncify_stop_unwind.call(&mut ctx);
        } else {
            warn!("failed to unwind the stack because the asyncify_start_rewind export is missing");
            return Ok(OnCalledAction::Finish);
        }

        Ok(callback(ctx, memory_stack, unwind_stack))
    });

    // We need to exit the function so that it can unwind and then invoke the callback
    Ok(Errno::Success)
}

#[must_use = "the action must be passed to the call loop"]
fn rewind<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    memory_stack: Bytes,
    rewind_stack: Bytes,
    store_data: Bytes,
) -> Errno {
    trace!(
        "wasi[{}:{}]::rewinding (memory_stack_size={}, rewind_size={}, store_data={})",
        ctx.data().pid(),
        ctx.data().tid(),
        memory_stack.len(),
        rewind_stack.len(),
        store_data.len()
    );

    // Store the memory stack so that it can be restored later
    super::REWIND.with(|cell| cell.replace(Some(memory_stack)));

    // Deserialize the store data back into a snapshot
    let store_snapshot = match StoreSnapshot::deserialize(&store_data[..]) {
        Ok(a) => a,
        Err(err) => {
            warn!("snapshot restore failed - the store snapshot could not be deserialized");
            return Errno::Fault;
        }
    };
    ctx.as_store_mut().restore_snapshot(&store_snapshot);
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    // Write the addresses to the start of the stack space
    let rewind_pointer: u64 = wasi_try!(env.stack_start.try_into().map_err(|_| Errno::Overflow));
    let rewind_data_start =
        rewind_pointer + (std::mem::size_of::<__wasi_asyncify_t<M::Offset>>() as u64);
    let rewind_data_end = rewind_data_start + (rewind_stack.len() as u64);
    if rewind_data_end > env.stack_base {
        warn!(
            "attempting to rewind a stack bigger than the allocated stack space ({} > {})",
            rewind_data_end, env.stack_base
        );
        return Errno::Overflow;
    }
    let rewind_data = __wasi_asyncify_t::<M::Offset> {
        start: wasi_try!(rewind_data_end.try_into().map_err(|_| Errno::Overflow)),
        end: wasi_try!(env.stack_base.try_into().map_err(|_| Errno::Overflow)),
    };
    let rewind_data_ptr: WasmPtr<__wasi_asyncify_t<M::Offset>, M> =
        WasmPtr::new(wasi_try!(rewind_pointer
            .try_into()
            .map_err(|_| Errno::Overflow)));
    wasi_try_mem!(rewind_data_ptr.write(&memory, rewind_data));

    // Copy the data to the address
    let rewind_stack_ptr = WasmPtr::<u8, M>::new(wasi_try!(rewind_data_start
        .try_into()
        .map_err(|_| Errno::Overflow)));
    wasi_try_mem!(rewind_stack_ptr
        .slice(
            &memory,
            wasi_try!(rewind_stack.len().try_into().map_err(|_| Errno::Overflow))
        )
        .and_then(|stack| { stack.write_slice(&rewind_stack[..]) }));

    // Invoke the callback that will prepare to rewind
    let asyncify_data = wasi_try!(rewind_pointer.try_into().map_err(|_| Errno::Overflow));
    if let Some(asyncify_start_rewind) = env.inner().asyncify_start_rewind.clone() {
        asyncify_start_rewind.call(&mut ctx, asyncify_data);
    } else {
        warn!("failed to rewind the stack because the asyncify_start_rewind export is missing");
        return Errno::Fault;
    }

    Errno::Success
}

#[cfg(feature = "os")]
fn handle_rewind<M: MemorySize>(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> bool {
    // If the stack has been restored
    if let Some(memory_stack) = super::REWIND.with(|cell| cell.borrow_mut().take()) {
        // Notify asyncify that we are no longer rewinding
        let env = ctx.data();
        if let Some(asyncify_stop_rewind) = env.inner().asyncify_stop_rewind.clone() {
            asyncify_stop_rewind.call(ctx);
        }

        // Restore the memory stack
        set_memory_stack::<M>(ctx, memory_stack);
        true
    } else {
        false
    }
}

/// ### `stack_checkpoint()`
/// Creates a snapshot of the current stack which allows it to be restored
/// later using its stack hash.
#[cfg(feature = "os")]
pub fn stack_checkpoint<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    snapshot_ptr: WasmPtr<StackSnapshot, M>,
    ret_val: WasmPtr<Longsize, M>,
) -> Result<Errno, WasiError> {
    // If we were just restored then we need to return the value instead
    if handle_rewind::<M>(&mut ctx) {
        let env = ctx.data();
        let memory = env.memory_view(&ctx);
        let ret_val = wasi_try_mem_ok!(ret_val.read(&memory));
        trace!(
            "wasi[{}:{}]::stack_checkpoint - restored - (ret={})",
            ctx.data().pid(),
            ctx.data().tid(),
            ret_val
        );
        return Ok(Errno::Success);
    }
    trace!(
        "wasi[{}:{}]::stack_checkpoint - capturing",
        ctx.data().pid(),
        ctx.data().tid()
    );

    // Set the return value that we will give back to
    // indicate we are a normal function call that has not yet
    // been restored
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(ret_val.write(&memory, 0));

    // Pass some offsets to the unwind function
    let ret_offset = ret_val.offset();
    let snapshot_offset = snapshot_ptr.offset();
    let secret = env.state().secret.clone();

    // We clear the target memory location before we grab the stack so that
    // it correctly hashes
    if let Err(err) = snapshot_ptr.write(&memory, StackSnapshot { hash: 0, user: 0 }) {
        warn!(
            "wasi[{}:{}]::failed to write to stack snapshot return variable - {}",
            env.pid(),
            env.tid(),
            err
        );
    }

    // Perform the unwind action
    unwind::<M, _>(ctx, move |mut ctx, mut memory_stack, rewind_stack| {
        // Grab all the globals and serialize them
        let env = ctx.data();
        let store_data = ctx.as_store_ref().save_snapshot().serialize();
        let store_data = Bytes::from(store_data);
        let mut memory_stack_corrected = memory_stack.clone();

        // We compute the hash again for two reasons... integrity so if there
        // is a long jump that goes to the wrong place it will fail gracefully.
        // and security so that the stack can not be used to attempt to break
        // out of the sandbox
        let hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&secret[..]);
            hasher.update(&memory_stack[..]);
            hasher.update(&rewind_stack[..]);
            hasher.update(&store_data[..]);
            let hash: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();
            u128::from_le_bytes(hash)
        };

        // Build a stack snapshot
        let snapshot = StackSnapshot {
            hash,
            user: ret_offset.into(),
        };

        // Get a reference directly to the bytes of snapshot
        let val_bytes = unsafe {
            let p = &snapshot;
            ::std::slice::from_raw_parts(
                (p as *const StackSnapshot) as *const u8,
                ::std::mem::size_of::<StackSnapshot>(),
            )
        };

        // The snapshot may itself reside on the stack (which means we
        // need to update the memory stack rather than write to the memory
        // as otherwise the rewind will wipe out the structure)
        // This correct memory stack is stored as well for validation purposes
        let mut memory_stack_corrected = memory_stack.clone();
        {
            let snapshot_offset: u64 = snapshot_offset.into();
            if snapshot_offset >= env.stack_start && snapshot_offset < env.stack_base {
                // Make sure its within the "active" part of the memory stack
                // (note - the area being written to might not go past the memory pointer)
                let offset = env.stack_base - snapshot_offset;
                if (offset as usize) < memory_stack_corrected.len() {
                    let left = memory_stack_corrected.len() - (offset as usize);
                    let end = offset + (val_bytes.len().min(left) as u64);
                    if end as usize <= memory_stack_corrected.len() {
                        let pstart = memory_stack_corrected.len() - offset as usize;
                        let pend = pstart + val_bytes.len();
                        let pbytes = &mut memory_stack_corrected[pstart..pend];
                        pbytes.clone_from_slice(&val_bytes);
                    }
                }
            }
        }

        /// Add a snapshot to the stack
        ctx.data().thread.add_snapshot(
            &memory_stack[..],
            &memory_stack_corrected[..],
            hash,
            &rewind_stack[..],
            &store_data[..],
        );
        trace!(
            "wasi[{}:{}]::stack_recorded (hash={}, user={})",
            ctx.data().pid(),
            ctx.data().tid(),
            snapshot.hash,
            snapshot.user
        );

        // Save the stack snapshot
        let env = ctx.data();
        let memory = env.memory_view(&ctx);
        let snapshot_ptr: WasmPtr<StackSnapshot, M> = WasmPtr::new(snapshot_offset);
        if let Err(err) = snapshot_ptr.write(&memory, snapshot) {
            warn!(
                "wasi[{}:{}]::failed checkpoint - could not save stack snapshot - {}",
                env.pid(),
                env.tid(),
                err
            );
            return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
        }

        // Rewind the stack and carry on
        let pid = ctx.data().pid();
        let tid = ctx.data().tid();
        match rewind::<M>(
            ctx,
            memory_stack_corrected.freeze(),
            rewind_stack.freeze(),
            store_data,
        ) {
            Errno::Success => OnCalledAction::InvokeAgain,
            err => {
                warn!(
                    "wasi[{}:{}]::failed checkpoint - could not rewind the stack - errno={}",
                    pid, tid, err
                );
                OnCalledAction::Trap(Box::new(WasiError::Exit(err as u32)))
            }
        }
    })
}

#[allow(unused_variables)]
#[cfg(not(feature = "os"))]
pub fn stack_checkpoint<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    snapshot_ptr: WasmPtr<StackSnapshot, M>,
    ret_val: WasmPtr<Longsize, M>,
) -> Result<Errno, WasiError> {
    warn!(
        "wasi[{}:{}]::stack_checkpoint - not supported without 'os' feature",
        ctx.data().pid(),
        ctx.data().tid()
    );
    return Ok(Errno::Notsup);
}

/// ### `stack_restore()`
/// Restores the current stack to a previous stack described by its
/// stack hash.
///
/// ## Parameters
///
/// * `snapshot_ptr` - Contains a previously made snapshot
#[cfg(feature = "os")]
pub fn stack_restore<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    snapshot_ptr: WasmPtr<StackSnapshot, M>,
    mut val: Longsize,
) -> Result<(), WasiError> {
    // Read the snapshot from the stack
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let snapshot = match snapshot_ptr.read(&memory) {
        Ok(a) => {
            trace!(
                "wasi[{}:{}]::stack_restore (with_ret={}, hash={}, user={})",
                ctx.data().pid(),
                ctx.data().tid(),
                val,
                a.hash,
                a.user
            );
            a
        }
        Err(err) => {
            warn!(
                "wasi[{}:{}]::stack_restore - failed to read stack snapshot - {}",
                ctx.data().pid(),
                ctx.data().tid(),
                err
            );
            return Err(WasiError::Exit(128));
        }
    };

    // Perform the unwind action
    unwind::<M, _>(ctx, move |mut ctx, _, _| {
        // Let the stack (or fail trying!)
        let env = ctx.data();
        if let Some((mut memory_stack, rewind_stack, store_data)) =
            env.thread.get_snapshot(snapshot.hash)
        {
            let env = ctx.data();
            let memory = env.memory_view(&ctx);

            // If the return value offset is within the memory stack then we need
            // to update it here rather than in the real memory
            let ret_val_offset = snapshot.user;
            if ret_val_offset >= env.stack_start && ret_val_offset < env.stack_base {
                // Make sure its within the "active" part of the memory stack
                let val_bytes = val.to_ne_bytes();
                let offset = env.stack_base - ret_val_offset;
                let end = offset + (val_bytes.len() as u64);
                if end as usize > memory_stack.len() {
                    warn!("wasi[{}:{}]::snapshot stack restore failed - the return value is outside of the active part of the memory stack ({} vs {}) - {} - {}", env.pid(), env.tid(), offset, memory_stack.len(), ret_val_offset, end);
                    return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
                } else {
                    // Update the memory stack with the new return value
                    let pstart = memory_stack.len() - offset as usize;
                    let pend = pstart + val_bytes.len();
                    let pbytes = &mut memory_stack[pstart..pend];
                    pbytes.clone_from_slice(&val_bytes);
                }
            } else {
                let err = snapshot
                    .user
                    .try_into()
                    .map_err(|_| Errno::Overflow)
                    .map(|a| WasmPtr::<Longsize, M>::new(a))
                    .map(|a| {
                        a.write(&memory, val)
                            .map(|_| Errno::Success)
                            .unwrap_or(Errno::Fault)
                    })
                    .unwrap_or_else(|a| a);
                if err != Errno::Success {
                    warn!("wasi[{}:{}]::snapshot stack restore failed - the return value can not be written too - {}", env.pid(), env.tid(), err);
                    return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
                }
            }

            // Rewind the stack - after this point we must immediately return
            // so that the execution can end here and continue elsewhere.
            let pid = ctx.data().pid();
            let tid = ctx.data().tid();
            match rewind::<M>(ctx, memory_stack.freeze(), rewind_stack, store_data) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!(
                        "wasi[{}:{}]::failed to rewind the stack - errno={}",
                        pid, tid, err
                    );
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
                }
            }
        } else {
            warn!("wasi[{}:{}]::snapshot stack restore failed - the snapshot can not be found and hence restored (hash={})", env.pid(), env.tid(), snapshot.hash);
            OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
        }
    });

    // Return so the stack can be unwound (which will then
    // be rewound again but with a different location)
    Ok(())
}

#[cfg(not(feature = "os"))]
pub fn stack_restore<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    snapshot_ptr: WasmPtr<StackSnapshot, M>,
    mut val: Longsize,
) -> Result<(), WasiError> {
    warn!(
        "wasi[{}:{}]::stack_restore - not supported without 'os' feature",
        ctx.data().pid(),
        ctx.data().tid()
    );
    Ok(())
}

/// ### `proc_signal()`
/// Sends a signal to a child process
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
/// * `sig` - Signal to send the child process
#[cfg(feature = "os")]
pub fn proc_signal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid: Pid,
    sig: Signal,
) -> Result<Errno, WasiError> {
    trace!(
        "wasi[{}:{}]::proc_signal(pid={}, sig={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        pid,
        sig
    );

    let process = {
        let pid: WasiProcessId = pid.into();
        ctx.data().process.compute.get_process(pid)
    };
    if let Some(process) = process {
        process.signal_process(sig);
    }

    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    Ok(Errno::Success)
}

#[cfg(not(feature = "os"))]
pub fn proc_signal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid: Pid,
    sig: Signal,
) -> Result<Errno, WasiError> {
    warn!(
        "wasi[{}:{}]::proc_signal(pid={}, sig={:?}) is not supported without 'os' feature",
        ctx.data().pid(),
        ctx.data().tid(),
        pid,
        sig
    );
    Ok(Errno::Notsup)
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
    trace!(
        "wasi[{}:{}]::random_get(buf_len={})",
        ctx.data().pid(),
        ctx.data().tid(),
        buf_len
    );
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
    debug!("wasi[{}:{}]::tty_get", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();

    let state = env.runtime.tty_get();
    let state = Tty {
        cols: state.cols,
        rows: state.rows,
        width: state.width,
        height: state.height,
        stdin_tty: match state.stdin_tty {
            false => Bool::False,
            true => Bool::True,
        },
        stdout_tty: match state.stdout_tty {
            false => Bool::False,
            true => Bool::True,
        },
        stderr_tty: match state.stderr_tty {
            false => Bool::False,
            true => Bool::True,
        },
        echo: match state.echo {
            false => Bool::False,
            true => Bool::True,
        },
        line_buffered: match state.line_buffered {
            false => Bool::False,
            true => Bool::True,
        },
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
    let echo = match state.echo {
        Bool::False => false,
        Bool::True => true,
    };
    let line_buffered = match state.line_buffered {
        Bool::False => false,
        Bool::True => true,
    };
    let line_feeds = true;
    debug!(
        "wasi[{}:{}]::tty_set(echo={}, line_buffered={}, line_feeds={})",
        ctx.data().pid(),
        ctx.data().tid(),
        echo,
        line_buffered,
        line_feeds
    );

    let state = super::runtime::WasiTtyState {
        cols: state.cols,
        rows: state.rows,
        width: state.width,
        height: state.height,
        stdin_tty: match state.stdin_tty {
            Bool::False => false,
            Bool::True => true,
        },
        stdout_tty: match state.stdout_tty {
            Bool::False => false,
            Bool::True => true,
        },
        stderr_tty: match state.stderr_tty {
            Bool::False => false,
            Bool::True => true,
        },
        echo,
        line_buffered,
        line_feeds,
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
    debug!("wasi[{}:{}]::getcwd", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let (memory, mut state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let (_, cur_dir) = wasi_try!(state
        .fs
        .get_current_dir(inodes.deref_mut(), crate::VIRTUAL_ROOT_FD,));
    trace!(
        "wasi[{}:{}]::getcwd(current_dir={})",
        ctx.data().pid(),
        ctx.data().tid(),
        cur_dir
    );

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
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let path = unsafe { get_input_str!(&memory, path, path_len) };
    debug!(
        "wasi[{}:{}]::chdir [{}]",
        ctx.data().pid(),
        ctx.data().tid(),
        path
    );

    // Check if the directory exists
    if state.fs.root_fs.read_dir(Path::new(path.as_str())).is_err() {
        return Errno::Noent;
    }

    state.fs.set_current_dir(path.as_str());
    Errno::Success
}

/// ### `callback_spawn()`
/// Sets the callback to invoke upon spawning of new threads
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
pub fn callback_thread<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), MemoryAccessError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let name = unsafe { name.read_utf8_string(&memory, name_len)? };
    debug!(
        "wasi[{}:{}]::callback_spawn (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    let funct = env.inner().exports.get_typed_function(&ctx, &name).ok();

    ctx.data_mut().inner_mut().thread_spawn = funct;
    Ok(())
}

/// ### `callback_signal()`
/// Sets the callback to invoke signals
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
pub fn callback_signal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let name = unsafe {
        match name.read_utf8_string(&memory, name_len) {
            Ok(a) => a,
            Err(err) => {
                warn!(
                    "failed to access memory that holds the name of the signal callback: {}",
                    err
                );
                return Ok(());
            }
        }
    };

    let funct = env.inner().exports.get_typed_function(&ctx, &name).ok();
    trace!(
        "wasi[{}:{}]::callback_signal (name={}, found={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name,
        funct.is_some()
    );

    {
        let inner = ctx.data_mut().inner_mut();
        inner.signal = funct;
        inner.signal_set = true;
    }

    let _ = ctx.data().clone().process_signals_and_exit(&mut ctx)?;

    Ok(())
}

/// ### `callback_reactor()`
/// Sets the callback to invoke for reactors
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
pub fn callback_reactor<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), MemoryAccessError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let name = unsafe { name.read_utf8_string(&memory, name_len)? };
    debug!(
        "wasi[{}:{}]::callback_reactor (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    let funct = env.inner().exports.get_typed_function(&ctx, &name).ok();

    ctx.data_mut().inner_mut().react = funct;
    Ok(())
}

/// ### `callback_thread_local_destroy()`
/// Sets the callback to invoke for the destruction of thread local variables
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
pub fn callback_thread_local_destroy<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), MemoryAccessError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let name = unsafe { name.read_utf8_string(&memory, name_len)? };
    debug!(
        "wasi[{}:{}]::callback_thread_local_destroy (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    let funct = env.inner().exports.get_typed_function(&ctx, &name).ok();

    ctx.data_mut().inner_mut().thread_local_destroy = funct;
    Ok(())
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
#[cfg(feature = "os")]
pub fn thread_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    user_data: u64,
    stack_base: u64,
    stack_start: u64,
    reactor: Bool,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::thread_spawn (reactor={:?}, thread_id={}, stack_base={}, caller_id={})",
        ctx.data().pid(),
        ctx.data().tid(),
        reactor,
        ctx.data().thread.tid().raw(),
        stack_base,
        current_caller_id().raw()
    );

    // Now we use the environment and memory references
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let runtime = env.runtime.clone();
    let tasks = env.tasks.clone();

    // Create the handle that represents this thread
    let mut thread_handle = env.process.new_thread();
    let thread_id: Tid = thread_handle.id().into();

    // We need a copy of the process memory and a packaged store in order to
    // launch threads and reactors
    let thread_memory = wasi_try!(ctx.data().memory().try_clone(&ctx).ok_or_else(|| {
        error!("thread failed - the memory could not be cloned");
        Errno::Notcapable
    }));
    #[cfg(feature = "compiler")]
    let engine = ctx.as_store_ref().engine().clone();

    // Build a new store that will be passed to the thread
    #[cfg(feature = "compiler")]
    let mut store = Store::new(engine);
    #[cfg(not(feature = "compiler"))]
    let mut store = Store::default();

    // This function takes in memory and a store and creates a context that
    // can be used to call back into the process
    let create_ctx = {
        let state = env.state.clone();
        let wasi_env = env.clone();
        let thread = thread_handle.as_thread();
        move |mut store: Store, module: Module, memory: VMMemory| {
            // We need to reconstruct some things
            let module = module.clone();
            let memory = Memory::new_from_existing(&mut store, memory);

            // Build the context object and import the memory
            let mut ctx = WasiFunctionEnv::new(&mut store, wasi_env.clone());
            {
                let env = ctx.data_mut(&mut store);
                env.thread = thread.clone();
                env.stack_base = stack_base;
                env.stack_start = stack_start;
            }

            let mut import_object = import_object_for_all_wasi_versions(&mut store, &ctx.env);
            import_object.define("env", "memory", memory.clone());

            let instance = match Instance::new(&mut store, &module, &import_object) {
                Ok(a) => a,
                Err(err) => {
                    error!("thread failed - create instance failed: {}", err);
                    return Err(Errno::Noexec as u32);
                }
            };

            // Set the current thread ID
            ctx.data_mut(&mut store).inner =
                Some(WasiEnvInner::new(module, memory, &store, &instance));
            trace!(
                "threading: new context created for thread_id = {}",
                thread.tid().raw()
            );
            Ok(WasiThreadContext {
                ctx,
                store: RefCell::new(store),
            })
        }
    };

    // This function calls into the module
    let call_module = move |ctx: &WasiFunctionEnv, store: &mut Store| {
        // We either call the reactor callback or the thread spawn callback
        //trace!("threading: invoking thread callback (reactor={})", reactor);
        let spawn = match reactor {
            Bool::False => ctx.data(&store).inner().thread_spawn.clone().unwrap(),
            Bool::True => ctx.data(&store).inner().react.clone().unwrap(),
            _ => {
                debug!("thread failed - failed as the reactor type is not value");
                return Errno::Noexec as u32;
            }
        };

        let user_data_low: u32 = (user_data & 0xFFFFFFFF) as u32;
        let user_data_high: u32 = (user_data >> 32) as u32;

        let mut ret = Errno::Success;
        if let Err(err) = spawn.call(store, user_data_low as i32, user_data_high as i32) {
            debug!("thread failed - start: {}", err);
            ret = Errno::Noexec;
        }
        //trace!("threading: thread callback finished (reactor={}, ret={})", reactor, ret);

        // If we are NOT a reactor then we will only run once and need to clean up
        if reactor == Bool::False {
            // Clean up the environment
            ctx.cleanup(store);
        }

        // Return the result
        ret as u32
    };

    // This next function gets a context for the local thread and then
    // calls into the process
    let mut execute_module = {
        let state = env.state.clone();
        move |store: &mut Option<Store>, module: Module, memory: &mut Option<VMMemory>| {
            // We capture the thread handle here, it is used to notify
            // anyone that is interested when this thread has terminated
            let _captured_handle = Box::new(&mut thread_handle);

            // Given that it is not safe to assume this delegate will run on the
            // same thread we need to capture a simple process that will create
            // context objects on demand and reuse them
            let caller_id = current_caller_id();

            // We loop because read locks are held while functions run which need
            // to be relocked in the case of a miss hit.
            loop {
                let thread = {
                    let guard = state.threading.read().unwrap();
                    guard.thread_ctx.get(&caller_id).map(|a| a.clone())
                };
                if let Some(thread) = thread {
                    let mut store = thread.store.borrow_mut();
                    let ret = call_module(&thread.ctx, store.deref_mut());
                    return ret;
                }

                // Otherwise we need to create a new context under a write lock
                debug!(
                    "encountered a new caller (ref={}) - creating WASM execution context...",
                    caller_id.raw()
                );

                // We can only create the context once per thread
                let memory = match memory.take() {
                    Some(m) => m,
                    None => {
                        debug!(
                            "thread failed - memory can only be consumed once per context creation"
                        );
                        return Errno::Noexec as u32;
                    }
                };
                let store = match store.take() {
                    Some(s) => s,
                    None => {
                        debug!(
                            "thread failed - store can only be consumed once per context creation"
                        );
                        return Errno::Noexec as u32;
                    }
                };

                // Now create the context and hook it up
                let mut guard = state.threading.write().unwrap();
                let ctx = match create_ctx(store, module.clone(), memory) {
                    Ok(c) => c,
                    Err(err) => {
                        return err;
                    }
                };
                guard.thread_ctx.insert(caller_id, Arc::new(ctx));
            }
        }
    };

    // If we are a reactor then instead of launching the thread now
    // we store it in the state machine and only launch it whenever
    // work arrives that needs to be processed
    match reactor {
        Bool::True => {
            warn!("thread failed - reactors are not currently supported");
            return Errno::Notcapable;
        }
        Bool::False => {
            // If the process does not export a thread spawn function then obviously
            // we can't spawn a background thread
            if env.inner().thread_spawn.is_none() {
                warn!("thread failed - the program does not export a _start_thread function");
                return Errno::Notcapable;
            }

            // Now spawn a thread
            trace!("threading: spawning background thread");
            let thread_module = env.inner().module.clone();
            wasi_try!(tasks
                .task_wasm(
                    Box::new(move |store, module, thread_memory| {
                        let mut thread_memory = thread_memory;
                        let mut store = Some(store);
                        execute_module(&mut store, module, &mut thread_memory);
                    }),
                    store,
                    thread_module,
                    crate::runtime::SpawnType::NewThread(thread_memory)
                )
                .map_err(|err| { Into::<Errno>::into(err) }));
        }
        _ => {
            warn!("thread failed - invalid reactor parameter value");
            return Errno::Notcapable;
        }
    }

    // Success
    let memory = ctx.data().memory_view(&ctx);
    wasi_try_mem!(ret_tid.write(&memory, thread_id));
    Errno::Success
}

#[cfg(not(feature = "os"))]
pub fn thread_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    user_data: u64,
    stack_base: u64,
    stack_start: u64,
    reactor: Bool,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    warn!(
        "wasi[{}:{}]::thread_spawn is not supported without 'os' feature",
        ctx.data().pid(),
        ctx.data().tid(),
    );
    Errno::Notsup
}

/// ### `thread_local_create()`
/// Create a thread local variable
/// If The web assembly process exports function named '_thread_local_destroy'
/// then it will be invoked when the thread goes out of scope and dies.
///
/// ## Parameters
///
/// * `user_data` - User data that will be passed to the destructor
///   when the thread variable goes out of scope
pub fn thread_local_create<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    user_data: TlUser,
    ret_key: WasmPtr<TlKey, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::thread_local_create (user_data={})",
        ctx.data().pid(),
        ctx.data().tid(),
        user_data
    );
    let env = ctx.data();

    let key = {
        let mut inner = env.process.write();
        inner.thread_local_seed += 1;
        let key = inner.thread_local_seed;
        inner.thread_local_user_data.insert(key, user_data);
        key
    };

    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_key.write(&memory, key));
    Errno::Success
}

/// ### `thread_local_destroy()`
/// Destroys a thread local variable
///
/// ## Parameters
///
/// * `user_data` - User data that will be passed to the destructor
///   when the thread variable goes out of scope
/// * `key` - Thread key that was previously created
pub fn thread_local_destroy(mut ctx: FunctionEnvMut<'_, WasiEnv>, key: TlKey) -> Errno {
    trace!(
        "wasi[{}:{}]::thread_local_destroy (key={})",
        ctx.data().pid(),
        ctx.data().tid(),
        key
    );
    let process = ctx.data().process.clone();
    let mut inner = process.write();

    let data = inner
        .thread_local
        .iter()
        .filter(|((_, k), _)| *k == key)
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>();
    inner.thread_local.retain(|(_, k), _| *k != key);

    if let Some(user_data) = inner.thread_local_user_data.remove(&key) {
        drop(inner);

        if let Some(thread_local_destroy) = ctx
            .data()
            .inner()
            .thread_local_destroy
            .as_ref()
            .map(|a| a.clone())
        {
            for val in data {
                let user_data_low: u32 = (user_data & 0xFFFFFFFF) as u32;
                let user_data_high: u32 = (user_data >> 32) as u32;

                let val_low: u32 = (val & 0xFFFFFFFF) as u32;
                let val_high: u32 = (val >> 32) as u32;

                let _ = thread_local_destroy.call(
                    &mut ctx,
                    user_data_low as i32,
                    user_data_high as i32,
                    val_low as i32,
                    val_high as i32,
                );
            }
        }
    }
    Errno::Success
}

/// ### `thread_local_set()`
/// Sets the value of a thread local variable
///
/// ## Parameters
///
/// * `key` - Thread key that this local variable will be associated with
/// * `val` - Value to be set for the thread local variable
pub fn thread_local_set(ctx: FunctionEnvMut<'_, WasiEnv>, key: TlKey, val: TlVal) -> Errno {
    //trace!("wasi[{}:{}]::thread_local_set (key={}, val={})", ctx.data().pid(), ctx.data().tid(), key, val);
    let env = ctx.data();

    let current_thread = ctx.data().thread.tid();
    let mut inner = env.process.write();
    inner.thread_local.insert((current_thread, key), val);
    Errno::Success
}

/// ### `thread_local_get()`
/// Gets the value of a thread local variable
///
/// ## Parameters
///
/// * `key` - Thread key that this local variable that was previous set
pub fn thread_local_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    key: TlKey,
    ret_val: WasmPtr<TlVal, M>,
) -> Errno {
    //trace!("wasi[{}:{}]::thread_local_get (key={})", ctx.data().pid(), ctx.data().tid(), key);
    let env = ctx.data();

    let val = {
        let current_thread = ctx.data().thread.tid();
        let guard = env.process.read();
        guard
            .thread_local
            .get(&(current_thread, key))
            .map(|a| a.clone())
    };
    let val = val.unwrap_or_default();
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_val.write(&memory, val));
    Errno::Success
}

/// ### `thread_sleep()`
/// Sends the current thread to sleep for a period of time
///
/// ## Parameters
///
/// * `duration` - Amount of time that the thread should sleep
pub fn thread_sleep(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    duration: Timestamp,
) -> Result<Errno, WasiError> {
    /*
    trace!(
        "wasi[{}:{}]::thread_sleep",
        ctx.data().pid(),
        ctx.data().tid()
    );
    */
    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);
    let env = ctx.data();

    #[cfg(feature = "sys-thread")]
    if duration == 0 {
        std::thread::yield_now();
    }

    if duration > 0 {
        let duration = Duration::from_nanos(duration as u64);
        let tasks = env.tasks.clone();
        wasi_try_ok!(__asyncify(&mut ctx, Some(duration), async move {
            // using an infinite async sleep here means we don't have to write the same event
            // handling loop code for signals and timeouts
            InfiniteSleep::default().await;
            unreachable!(
                "the timeout or signals will wake up this thread even though it waits forever"
            )
        }));
    }
    Ok(Errno::Success)
}

/// ### `thread_id()`
/// Returns the index of the current thread
/// (threads indices are sequencial from zero)
pub fn thread_id<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    ret_tid: WasmPtr<Tid, M>,
) -> Errno {
    /*
    trace!(
        "wasi[{}:{}]::thread_id",
        ctx.data().pid(),
        ctx.data().tid()
    );
    */

    let env = ctx.data();
    let tid: Tid = env.thread.tid().into();
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
pub fn thread_join(mut ctx: FunctionEnvMut<'_, WasiEnv>, tid: Tid) -> Result<Errno, WasiError> {
    debug!("wasi::thread_join");
    debug!(
        "wasi[{}:{}]::thread_join(tid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        tid
    );

    let env = ctx.data();
    let tid: WasiThreadId = tid.into();
    let other_thread = env.process.get_thread(&tid);
    if let Some(other_thread) = other_thread {
        wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            other_thread.join().await;
            Ok(())
        }));
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
    debug!(
        "wasi[{}:{}]::thread_parallelism",
        ctx.data().pid(),
        ctx.data().tid()
    );

    let env = ctx.data();
    let parallelism = wasi_try!(env.tasks.thread_parallelism().map_err(|err| {
        let err: Errno = err.into();
        err
    }));
    let parallelism: M::Offset = wasi_try!(parallelism.try_into().map_err(|_| Errno::Overflow));
    let memory = env.memory_view(&ctx);
    wasi_try_mem!(ret_parallelism.write(&memory, parallelism));
    Errno::Success
}

/// Wait for a futex_wake operation to wake us.
/// Returns with EINVAL if the futex doesn't hold the expected value.
/// Returns false on timeout, and true in all other cases.
///
/// ## Parameters
///
/// * `futex` - Memory location that holds the value that will be checked
/// * `expected` - Expected value that should be currently held at the memory location
/// * `timeout` - Timeout should the futex not be triggered in the allocated time
pub fn futex_wait<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    futex_ptr: WasmPtr<u32, M>,
    expected: u32,
    timeout: WasmPtr<OptionTimestamp, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Result<Errno, WasiError> {
    trace!(
        "wasi[{}:{}]::futex_wait(offset={})",
        ctx.data().pid(),
        ctx.data().tid(),
        futex_ptr.offset()
    );
    let mut env = ctx.data();
    let state = env.state.clone();

    let pointer: u64 = wasi_try_ok!(futex_ptr.offset().try_into().map_err(|_| Errno::Overflow));

    // Register the waiting futex (if its not already registered)
    let futex = {
        use std::collections::hash_map::Entry;
        let mut guard = state.futexs.lock().unwrap();
        match guard.entry(pointer) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                let futex = WasiFutex {
                    refcnt: Arc::new(AtomicU32::new(1)),
                    inner: Arc::new(Mutex::new(tokio::sync::broadcast::channel(1).0)),
                };
                entry.insert(futex.clone());
                futex
            }
        }
    };

    // Determine the timeout
    let timeout = {
        let memory = env.memory_view(&ctx);
        wasi_try_mem_ok!(timeout.read(&memory))
    };
    let timeout = match timeout.tag {
        OptionTag::Some => Some(timeout.u as u128),
        _ => None,
    };

    // Loop until we either hit a yield error or the futex is woken
    let mut woken = Bool::False;
    let start = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1).unwrap() as u128;
    loop {
        let mut rx = {
            let futex_lock = futex.inner.lock().unwrap();
            // If the value of the memory is no longer the expected value
            // then terminate from the loop (we do this under a futex lock
            // so that its protected)
            {
                let view = env.memory_view(&ctx);
                let val = wasi_try_mem_ok!(futex_ptr.read(&view));
                if val != expected {
                    woken = Bool::True;
                    break;
                }
            }
            futex_lock.subscribe()
        };

        // Check if we have timed out
        let mut sub_timeout = None;
        if let Some(timeout) = timeout.as_ref() {
            let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1).unwrap() as u128;
            let delta = now.checked_sub(start).unwrap_or(0);
            if delta >= *timeout {
                break;
            }
            let remaining = *timeout - delta;
            sub_timeout = Some(Duration::from_nanos(remaining as u64));
        }

        // Now wait for it to be triggered
        wasi_try_ok!(__asyncify(&mut ctx, sub_timeout, async move {
            let _ = rx.recv().await;
            Ok(())
        }));
        env = ctx.data();
    }

    // Drop the reference count to the futex (and remove it if the refcnt hits zero)
    {
        let mut guard = state.futexs.lock().unwrap();
        if guard
            .get(&pointer)
            .map(|futex| futex.refcnt.fetch_sub(1, Ordering::AcqRel) == 1)
            .unwrap_or(false)
        {
            guard.remove(&pointer);
        }
    }

    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(ret_woken.write(&memory, woken));

    Ok(Errno::Success)
}

/// Wake up one thread that's blocked on futex_wait on this futex.
/// Returns true if this actually woke up such a thread,
/// or false if no thread was waiting on this futex.
///
/// ## Parameters
///
/// * `futex` - Memory location that holds a futex that others may be waiting on
pub fn futex_wake<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    futex: WasmPtr<u32, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::futex_wake(offset={})",
        ctx.data().pid(),
        ctx.data().tid(),
        futex.offset()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let state = env.state.deref();

    let pointer: u64 = wasi_try!(futex.offset().try_into().map_err(|_| Errno::Overflow));
    let mut woken = false;

    let mut guard = state.futexs.lock().unwrap();
    if let Some(futex) = guard.get(&pointer) {
        let inner = futex.inner.lock().unwrap();
        woken = inner.receiver_count() > 0;
        let _ = inner.send(());
    } else {
        trace!(
            "wasi[{}:{}]::futex_wake - nothing waiting!",
            ctx.data().pid(),
            ctx.data().tid()
        );
    }

    let woken = match woken {
        false => Bool::False,
        true => Bool::True,
    };
    wasi_try_mem!(ret_woken.write(&memory, woken));

    Errno::Success
}

/// Wake up all threads that are waiting on futex_wait on this futex.
///
/// ## Parameters
///
/// * `futex` - Memory location that holds a futex that others may be waiting on
pub fn futex_wake_all<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    futex: WasmPtr<u32, M>,
    ret_woken: WasmPtr<Bool, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::futex_wake_all(offset={})",
        ctx.data().pid(),
        ctx.data().tid(),
        futex.offset()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let state = env.state.deref();

    let pointer: u64 = wasi_try!(futex.offset().try_into().map_err(|_| Errno::Overflow));
    let mut woken = false;

    let mut guard = state.futexs.lock().unwrap();
    if let Some(futex) = guard.remove(&pointer) {
        let inner = futex.inner.lock().unwrap();
        woken = inner.receiver_count() > 0;
        let _ = inner.send(());
    }

    let woken = match woken {
        false => Bool::False,
        true => Bool::True,
    };
    wasi_try_mem!(ret_woken.write(&memory, woken));

    Errno::Success
}

/// ### `proc_id()`
/// Returns the handle of the current process
pub fn proc_id<M: MemorySize>(ctx: FunctionEnvMut<'_, WasiEnv>, ret_pid: WasmPtr<Pid, M>) -> Errno {
    debug!("wasi[{}:{}]::getpid", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let pid = env.process.pid();
    wasi_try_mem!(ret_pid.write(&memory, pid.raw() as Pid));
    Errno::Success
}

/// ### `proc_parent()`
/// Returns the parent handle of the supplied process
pub fn proc_parent<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    pid: Pid,
    ret_parent: WasmPtr<Pid, M>,
) -> Errno {
    debug!("wasi[{}:{}]::getppid", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let pid: WasiProcessId = pid.into();
    if pid == env.process.pid() {
        let memory = env.memory_view(&ctx);
        wasi_try_mem!(ret_parent.write(&memory, env.process.ppid().raw() as Pid));
    } else {
        let control_plane = env.process.control_plane();
        if let Some(process) = control_plane.get_process(pid) {
            let memory = env.memory_view(&ctx);
            wasi_try_mem!(ret_parent.write(&memory, process.pid().raw() as Pid));
        } else {
            return Errno::Badf;
        }
    }
    Errno::Success
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
    exitcode: ExitCode,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::thread_exit",
        ctx.data().pid(),
        ctx.data().tid()
    );
    Err(WasiError::Exit(exitcode))
}

// Function to prepare the WASI environment
fn _prepare_wasi(wasi_env: &mut WasiEnv, args: Option<Vec<String>>) {
    // Swap out the arguments with the new ones
    if let Some(args) = args {
        let mut wasi_state = wasi_env.state.fork();
        wasi_state.args = args;
        wasi_env.state = Arc::new(wasi_state);
    }

    // Close any files after the STDERR that are not preopened
    let close_fds = {
        let preopen_fds = {
            let preopen_fds = wasi_env.state.fs.preopen_fds.read().unwrap();
            preopen_fds.iter().map(|a| *a).collect::<HashSet<_>>()
        };
        let mut fd_map = wasi_env.state.fs.fd_map.read().unwrap();
        fd_map
            .keys()
            .filter_map(|a| match *a {
                a if a <= __WASI_STDERR_FILENO => None,
                a if preopen_fds.contains(&a) => None,
                a => Some(a),
            })
            .collect::<Vec<_>>()
    };

    // Now close all these files
    for fd in close_fds {
        let inodes = wasi_env.state.inodes.read().unwrap();
        let _ = wasi_env.state.fs.close_fd(inodes.deref(), fd);
    }
}

fn conv_bus_err_to_exit_code(err: VirtualBusError) -> u32 {
    match err {
        VirtualBusError::AccessDenied => -1i32 as u32,
        VirtualBusError::NotFound => -2i32 as u32,
        VirtualBusError::Unsupported => -22i32 as u32,
        VirtualBusError::BadRequest | _ => -8i32 as u32,
    }
}

/// ### `proc_fork()`
/// Forks the current process into a new subprocess. If the function
/// returns a zero then its the new subprocess. If it returns a positive
/// number then its the current process and the $pid represents the child.
#[cfg(feature = "os")]
pub fn proc_fork<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    mut copy_memory: Bool,
    pid_ptr: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    // If we were just restored then we need to return the value instead
    let fork_op = if copy_memory == Bool::True {
        "fork"
    } else {
        "vfork"
    };
    if handle_rewind::<M>(&mut ctx) {
        let env = ctx.data();
        let memory = env.memory_view(&ctx);
        let ret_pid = wasi_try_mem_ok!(pid_ptr.read(&memory));
        if ret_pid == 0 {
            trace!(
                "wasi[{}:{}]::proc_{} - entering child",
                ctx.data().pid(),
                ctx.data().tid(),
                fork_op
            );
        } else {
            trace!(
                "wasi[{}:{}]::proc_{} - entering parent(child={})",
                ctx.data().pid(),
                ctx.data().tid(),
                fork_op,
                ret_pid
            );
        }
        return Ok(Errno::Success);
    }
    trace!(
        "wasi[{}:{}]::proc_{} - capturing",
        ctx.data().pid(),
        ctx.data().tid(),
        fork_op
    );

    // Fork the environment which will copy all the open file handlers
    // and associate a new context but otherwise shares things like the
    // file system interface. The handle to the forked process is stored
    // in the parent process context
    let (mut child_env, mut child_handle) = ctx.data().fork();
    let child_pid = child_env.process.pid();

    // We write a zero to the PID before we capture the stack
    // so that this is what will be returned to the child
    {
        let mut children = ctx.data().process.children.write().unwrap();
        children.push(child_pid);
    }
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(pid_ptr.write(&memory, 0));

    // Pass some offsets to the unwind function
    let pid_offset = pid_ptr.offset();

    // If we are not copying the memory then we act like a `vfork`
    // instead which will pretend to be the new process for a period
    // of time until `proc_exec` is called at which point the fork
    // actually occurs
    if copy_memory == Bool::False {
        // Perform the unwind action
        let pid_offset: u64 = pid_offset.into();
        return unwind::<M, _>(ctx, move |mut ctx, mut memory_stack, rewind_stack| {
            // Grab all the globals and serialize them
            let store_data = ctx.as_store_ref().save_snapshot().serialize();
            let store_data = Bytes::from(store_data);

            // We first fork the environment and replace the current environment
            // so that the process can continue to prepare for the real fork as
            // if it had actually forked
            std::mem::swap(&mut ctx.data_mut().inner, &mut child_env.inner);
            std::mem::swap(ctx.data_mut(), &mut child_env);
            ctx.data_mut().vfork.replace(WasiVFork {
                rewind_stack: rewind_stack.clone(),
                memory_stack: memory_stack.clone(),
                store_data: store_data.clone(),
                env: Box::new(child_env),
                handle: child_handle,
                pid_offset,
            });

            // Carry on as if the fork had taken place (which basically means
            // it prevents to be the new process with the old one suspended)
            // Rewind the stack and carry on
            match rewind::<M>(
                ctx,
                memory_stack.freeze(),
                rewind_stack.freeze(),
                store_data,
            ) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!(
                        "{} failed - could not rewind the stack - errno={}",
                        fork_op, err
                    );
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
                }
            }
        });
    }

    // Create the thread that will back this forked process
    let state = env.state.clone();
    let bin_factory = env.bin_factory.clone();

    // Perform the unwind action
    unwind::<M, _>(ctx, move |mut ctx, mut memory_stack, rewind_stack| {
        // Grab all the globals and serialize them
        let store_data = ctx.as_store_ref().save_snapshot().serialize();
        let store_data = Bytes::from(store_data);

        // Fork the memory and copy the module (compiled code)
        let env = ctx.data();
        let fork_memory: VMMemory = match env
            .memory()
            .try_clone(&ctx)
            .ok_or_else(|| {
                error!(
                    "wasi[{}:{}]::{} failed - the memory could not be cloned",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    fork_op
                );
                MemoryError::Generic(format!("the memory could not be cloned"))
            })
            .and_then(|mut memory| memory.fork())
        {
            Ok(memory) => memory.into(),
            Err(err) => {
                warn!(
                    "wasi[{}:{}]::{} failed - could not fork the memory - {}",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    fork_op,
                    err
                );
                return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
            }
        };
        let fork_module = env.inner().module.clone();

        #[cfg(feature = "compiler")]
        let engine = ctx.as_store_ref().engine().clone();

        // Build a new store that will be passed to the thread
        #[cfg(feature = "compiler")]
        let mut fork_store = Store::new(engine);
        #[cfg(not(feature = "compiler"))]
        let mut fork_store = Store::default();

        // Now we use the environment and memory references
        let runtime = child_env.runtime.clone();
        let tasks = child_env.tasks.clone();
        let child_memory_stack = memory_stack.clone();
        let child_rewind_stack = rewind_stack.clone();

        // Spawn a new process with this current execution environment
        let signaler = Box::new(child_env.process.clone());
        let (exit_code_tx, exit_code_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let store_data = store_data.clone();
            let runtime = runtime.clone();
            let tasks = tasks.clone();
            let tasks_outer = tasks.clone();
            tasks_outer.task_wasm(Box::new(move |mut store, module, memory|
                {
                    // Create the WasiFunctionEnv
                    let pid = child_env.pid();
                    let tid = child_env.tid();
                    child_env.runtime = runtime.clone();
                    child_env.tasks = tasks.clone();
                    let mut ctx = WasiFunctionEnv::new(&mut store, child_env);

                    // Let's instantiate the module with the imports.
                    let mut import_object = import_object_for_all_wasi_versions(&mut store, &ctx.env);
                    let memory = if let Some(memory) = memory {
                        let memory = Memory::new_from_existing(&mut store, memory);
                        import_object.define("env", "memory", memory.clone());
                        memory
                    } else {
                        error!("wasi[{}:{}]::wasm instantiate failed - no memory supplied", pid, tid);
                        return;
                    };
                    let instance = match Instance::new(&mut store, &module, &import_object) {
                        Ok(a) => a,
                        Err(err) => {
                            error!("wasi[{}:{}]::wasm instantiate error ({})", pid, tid, err);
                            return;
                        }
                    };

                    // Set the current thread ID
                    ctx.data_mut(&mut store).inner = Some(
                        WasiEnvInner::new(module, memory, &store, &instance)
                    );

                    // Rewind the stack and carry on
                    {
                        trace!("wasi[{}:{}]::{}: rewinding child", ctx.data(&store).pid(), ctx.data(&store).tid(), fork_op);
                        let ctx = ctx.env.clone().into_mut(&mut store);
                        match rewind::<M>(ctx, child_memory_stack.freeze(), child_rewind_stack.freeze(), store_data.clone()) {
                            Errno::Success => OnCalledAction::InvokeAgain,
                            err => {
                                warn!("wasi[{}:{}]::wasm rewind failed - could not rewind the stack - errno={}", pid, tid, err);
                                return;
                            }
                        };
                    }

                    // Invoke the start function
                    let mut ret = Errno::Success;
                    if ctx.data(&store).thread.is_main() {
                        trace!("wasi[{}:{}]::{}: re-invoking main", ctx.data(&store).pid(), ctx.data(&store).tid(), fork_op);
                        let start = ctx.data(&store).inner().start.clone().unwrap();
                        start.call(&mut store);
                    } else {
                        trace!("wasi[{}:{}]::{}: re-invoking thread_spawn", ctx.data(&store).pid(), ctx.data(&store).tid(), fork_op);
                        let start = ctx.data(&store).inner().thread_spawn.clone().unwrap();
                        start.call(&mut store, 0, 0);
                    }

                    // Clean up the environment
                    ctx.cleanup((&mut store));

                    // Send the result
                    let _ = exit_code_tx.send(ret as u32);
                    drop(exit_code_tx);
                    drop(child_handle);
                }
            ), fork_store, fork_module, SpawnType::NewThread(fork_memory))
                .map_err(|err| {
                    warn!("wasi[{}:{}]::failed to fork as the process could not be spawned - {}", ctx.data().pid(), ctx.data().tid(), err);
                    err
                })
                .ok()
        };

        // Add the process to the environment state
        let process = BusSpawnedProcess {
            inst: Box::new(crate::bin_factory::SpawnedProcess {
                exit_code: Mutex::new(None),
                exit_code_rx: Mutex::new(exit_code_rx),
            }),
            stdin: None,
            stdout: None,
            stderr: None,
            signaler: Some(signaler),
        };
        {
            trace!(
                "wasi[{}:{}]::spawned sub-process (pid={})",
                ctx.data().pid(),
                ctx.data().tid(),
                child_pid.raw()
            );
            let mut inner = ctx.data().process.write();
            inner
                .bus_processes
                .insert(child_pid.into(), Box::new(process));
        }

        // If the return value offset is within the memory stack then we need
        // to update it here rather than in the real memory
        let pid_offset: u64 = pid_offset.into();
        if pid_offset >= env.stack_start && pid_offset < env.stack_base {
            // Make sure its within the "active" part of the memory stack
            let offset = env.stack_base - pid_offset;
            if offset as usize > memory_stack.len() {
                warn!("{} failed - the return value (pid) is outside of the active part of the memory stack ({} vs {})", fork_op, offset, memory_stack.len());
                return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
            }

            // Update the memory stack with the new PID
            let val_bytes = child_pid.raw().to_ne_bytes();
            let pstart = memory_stack.len() - offset as usize;
            let pend = pstart + val_bytes.len();
            let pbytes = &mut memory_stack[pstart..pend];
            pbytes.clone_from_slice(&val_bytes);
        } else {
            warn!("{} failed - the return value (pid) is not being returned on the stack - which is not supported", fork_op);
            return OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)));
        }

        // Rewind the stack and carry on
        match rewind::<M>(
            ctx,
            memory_stack.freeze(),
            rewind_stack.freeze(),
            store_data,
        ) {
            Errno::Success => OnCalledAction::InvokeAgain,
            err => {
                warn!(
                    "{} failed - could not rewind the stack - errno={}",
                    fork_op, err
                );
                OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
            }
        }
    })
}

#[allow(unused_variables)]
#[cfg(not(feature = "os"))]
pub fn proc_fork<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    mut copy_memory: Bool,
    pid_ptr: WasmPtr<Pid, M>,
) -> Result<Errno, WasiError> {
    warn!(
        "wasi[{}:{}]::proc_fork - not supported without 'os' feature",
        ctx.data().pid(),
        ctx.data().tid()
    );
    Ok(Errno::Notsup)
}

/// Replaces the current process with a new process
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `args` - List of the arguments to pass the process
///   (entries are separated by line feeds)
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
#[cfg(feature = "os")]
pub fn proc_exec<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    args: WasmPtr<u8, M>,
    args_len: M::Offset,
) -> Result<(), WasiError> {
    let memory = ctx.data().memory_view(&ctx);
    let mut name = name.read_utf8_string(&memory, name_len).map_err(|err| {
        warn!("failed to execve as the name could not be read - {}", err);
        WasiError::Exit(Errno::Fault as ExitCode)
    })?;
    trace!(
        "wasi[{}:{}]::proc_exec (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    let args = args.read_utf8_string(&memory, args_len).map_err(|err| {
        warn!("failed to execve as the args could not be read - {}", err);
        WasiError::Exit(Errno::Fault as ExitCode)
    })?;
    let args: Vec<_> = args
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| a.len() > 0)
        .collect();

    // Convert relative paths into absolute paths
    if name.starts_with("./") {
        name = ctx.data().state.fs.relative_path_to_absolute(name);
        trace!(
            "wasi[{}:{}]::rel_to_abs (name={}))",
            ctx.data().pid(),
            ctx.data().tid(),
            name
        );
    }

    // Convert the preopen directories
    let preopen = ctx.data().state.preopen.clone();

    // Get the current working directory
    let (_, cur_dir) = {
        let (memory, state, mut inodes) =
            ctx.data().get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);
        match state
            .fs
            .get_current_dir(inodes.deref_mut(), crate::VIRTUAL_ROOT_FD)
        {
            Ok(a) => a,
            Err(err) => {
                warn!("failed to create subprocess for fork - {}", err);
                return Err(WasiError::Exit(Errno::Fault as ExitCode));
            }
        }
    };

    // Build a new store that will be passed to the thread
    #[cfg(feature = "compiler")]
    let engine = ctx.as_store_ref().engine().clone();
    #[cfg(feature = "compiler")]
    let new_store = Store::new(engine);
    #[cfg(not(feature = "compiler"))]
    let new_store = Store::default();

    // If we are in a vfork we need to first spawn a subprocess of this type
    // with the forked WasiEnv, then do a longjmp back to the vfork point.
    if let Some(mut vfork) = ctx.data_mut().vfork.take() {
        // We will need the child pid later
        let child_pid = ctx.data().process.pid();

        // Restore the WasiEnv to the point when we vforked
        std::mem::swap(&mut vfork.env.inner, &mut ctx.data_mut().inner);
        std::mem::swap(vfork.env.as_mut(), ctx.data_mut());
        let mut wasi_env = *vfork.env;
        wasi_env.owned_handles.push(vfork.handle);
        _prepare_wasi(&mut wasi_env, Some(args));

        // Recrod the stack offsets before we give up ownership of the wasi_env
        let stack_base = wasi_env.stack_base;
        let stack_start = wasi_env.stack_start;

        // Spawn a new process with this current execution environment
        let mut err_exit_code = -2i32 as u32;
        let bus = ctx.data().bus();
        let mut process = bus
            .spawn(wasi_env)
            .spawn(
                Some(&ctx),
                name.as_str(),
                new_store,
                &ctx.data().bin_factory,
            )
            .map_err(|err| {
                err_exit_code = conv_bus_err_to_exit_code(err);
                warn!(
                    "failed to execve as the process could not be spawned (vfork) - {}",
                    err
                );
                let _ = stderr_write(
                    &ctx,
                    format!("wasm execute failed [{}] - {}\n", name.as_str(), err).as_bytes(),
                );
                err
            })
            .ok();

        // If no process was created then we create a dummy one so that an
        // exit code can be processed
        let process = match process {
            Some(a) => a,
            None => {
                debug!(
                    "wasi[{}:{}]::process failed with (err={})",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    err_exit_code
                );
                BusSpawnedProcess::exited_process(err_exit_code)
            }
        };

        // Add the process to the environment state
        {
            trace!(
                "wasi[{}:{}]::spawned sub-process (pid={})",
                ctx.data().pid(),
                ctx.data().tid(),
                child_pid.raw()
            );
            let mut inner = ctx.data().process.write();
            inner
                .bus_processes
                .insert(child_pid.into(), Box::new(process));
        }

        let mut memory_stack = vfork.memory_stack;
        let rewind_stack = vfork.rewind_stack;
        let store_data = vfork.store_data;

        // If the return value offset is within the memory stack then we need
        // to update it here rather than in the real memory
        let pid_offset: u64 = vfork.pid_offset.into();
        if pid_offset >= stack_start && pid_offset < stack_base {
            // Make sure its within the "active" part of the memory stack
            let offset = stack_base - pid_offset;
            if offset as usize > memory_stack.len() {
                warn!("vfork failed - the return value (pid) is outside of the active part of the memory stack ({} vs {})", offset, memory_stack.len());
            } else {
                // Update the memory stack with the new PID
                let val_bytes = child_pid.raw().to_ne_bytes();
                let pstart = memory_stack.len() - offset as usize;
                let pend = pstart + val_bytes.len();
                let pbytes = &mut memory_stack[pstart..pend];
                pbytes.clone_from_slice(&val_bytes);
            }
        } else {
            warn!("vfork failed - the return value (pid) is not being returned on the stack - which is not supported");
        }

        // Jump back to the vfork point and current on execution
        unwind::<M, _>(ctx, move |mut ctx, _, _| {
            // Rewind the stack
            match rewind::<M>(
                ctx,
                memory_stack.freeze(),
                rewind_stack.freeze(),
                store_data,
            ) {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!("fork failed - could not rewind the stack - errno={}", err);
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Fault as u32)))
                }
            }
        })?;
        return Ok(());
    }
    // Otherwise we need to unwind the stack to get out of the current executing
    // callstack, steal the memory/WasiEnv and switch it over to a new thread
    // on the new module
    else {
        // We need to unwind out of this process and launch a new process in its place
        unwind::<M, _>(ctx, move |mut ctx, _, _| {
            // Grab a reference to the bus
            let bus = ctx.data().bus().clone();

            // Prepare the environment
            let mut wasi_env = ctx.data_mut().clone();
            _prepare_wasi(&mut wasi_env, Some(args));

            // Get a reference to the runtime
            let bin_factory = ctx.data().bin_factory.clone();
            let tasks = wasi_env.tasks.clone();

            // Create the process and drop the context
            let builder = ctx.data().bus().spawn(wasi_env);

            // Spawn a new process with this current execution environment
            //let pid = wasi_env.process.pid();
            match builder.spawn(Some(&ctx), name.as_str(), new_store, &bin_factory) {
                Ok(mut process) => {
                    // Wait for the sub-process to exit itself - then we will exit
                    let (tx, rx) = std::sync::mpsc::channel();
                    let tasks_inner = tasks.clone();
                    tasks.block_on(Box::pin(async move {
                        loop {
                            tasks_inner.sleep_now(current_caller_id(), 5).await;
                            if let Some(exit_code) = process.inst.exit_code() {
                                tx.send(exit_code).unwrap();
                                break;
                            }
                        }
                    }));
                    let exit_code = rx.recv().unwrap();
                    return OnCalledAction::Trap(Box::new(WasiError::Exit(exit_code as ExitCode)));
                }
                Err(err) => {
                    warn!(
                        "failed to execve as the process could not be spawned (fork) - {}",
                        err
                    );
                    let exit_code = conv_bus_err_to_exit_code(err);
                    OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Noexec as ExitCode)))
                }
            }
        })?;
    }

    // Success
    Ok(())
}

#[cfg(not(feature = "os"))]
pub fn proc_exec<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    _name: WasmPtr<u8, M>,
    _name_len: M::Offset,
    _args: WasmPtr<u8, M>,
    _args_len: M::Offset,
) -> Result<(), WasiError> {
    warn!(
        "wasi[{}:{}]::exec is not supported in this build",
        ctx.data().pid(),
        ctx.data().tid()
    );
    Err(WasiError::Exit(Errno::Notsup as ExitCode))
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
#[cfg(feature = "os")]
pub fn proc_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
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
    let control_plane = env.process.control_plane();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus!(&memory, name, name_len) };
    let args = unsafe { get_input_str_bus!(&memory, args, args_len) };
    let preopen = unsafe { get_input_str_bus!(&memory, preopen, preopen_len) };
    let working_dir = unsafe { get_input_str_bus!(&memory, working_dir, working_dir_len) };
    debug!(
        "wasi[{}:{}]::process_spawn (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    if chroot == Bool::True {
        warn!(
            "wasi[{}:{}]::chroot is not currently supported",
            ctx.data().pid(),
            ctx.data().tid()
        );
        return BusErrno::Unsupported;
    }

    let args: Vec<_> = args
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| a.len() > 0)
        .collect();

    let preopen: Vec<_> = preopen
        .split(&['\n', '\r'])
        .map(|a| a.to_string())
        .filter(|a| a.len() > 0)
        .collect();

    let (handles, ctx) = match proc_spawn_internal(
        ctx,
        name,
        Some(args),
        Some(preopen),
        Some(working_dir),
        stdin,
        stdout,
        stderr,
    ) {
        Ok(a) => a,
        Err(err) => {
            return err;
        }
    };

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem_bus!(ret_handles.write(&memory, handles));
    BusErrno::Success
}

#[allow(unused_variables)]
#[cfg(not(feature = "os"))]
pub fn proc_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
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
    warn!(
        "wasi[{}:{}]::spawn is not supported on this platform",
        ctx.data().pid(),
        ctx.data().tid()
    );
    BusErrno::Unsupported
}

#[cfg(not(feature = "os"))]
pub fn proc_spawn_internal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    _name: String,
    _args: Option<Vec<String>>,
    _preopen: Option<Vec<String>>,
    _working_dir: Option<String>,
    _stdin: StdioMode,
    _stdout: StdioMode,
    _stderr: StdioMode,
) -> Result<(BusHandles, FunctionEnvMut<'_, WasiEnv>), BusErrno> {
    warn!(
        "wasi[{}:{}]::spawn is not supported on this platform",
        ctx.data().pid(),
        ctx.data().tid()
    );
    Err(BusErrno::Unsupported)
}

#[cfg(feature = "os")]
pub fn proc_spawn_internal(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: String,
    args: Option<Vec<String>>,
    preopen: Option<Vec<String>>,
    working_dir: Option<String>,
    stdin: WasiStdioMode,
    stdout: WasiStdioMode,
    stderr: WasiStdioMode,
) -> Result<(BusHandles, FunctionEnvMut<'_, WasiEnv>), BusErrno> {
    use crate::WasiPipe;

    let env = ctx.data();

    // Build a new store that will be passed to the thread
    #[cfg(feature = "compiler")]
    let engine = ctx.as_store_ref().engine().clone();
    #[cfg(feature = "compiler")]
    let new_store = Store::new(engine);
    #[cfg(not(feature = "compiler"))]
    let new_store = Store::default();

    // Fork the current environment and set the new arguments
    let (mut child_env, handle) = ctx.data().fork();
    if let Some(args) = args {
        let mut child_state = env.state.fork();
        child_state.args = args;
        child_env.state = Arc::new(child_state);
    }

    // Take ownership of this child
    ctx.data_mut().owned_handles.push(handle);
    let env = ctx.data();

    // Preopen
    if let Some(preopen) = preopen {
        if preopen.is_empty() == false {
            for preopen in preopen {
                warn!(
                    "wasi[{}:{}]::preopens are not yet supported for spawned processes [{}]",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    preopen
                );
            }
            return Err(BusErrno::Unsupported);
        }
    }

    // Change the current directory
    if let Some(working_dir) = working_dir {
        child_env.state.fs.set_current_dir(working_dir.as_str());
    }

    // Replace the STDIO
    let (stdin, stdout, stderr) = {
        let (_, child_state, mut child_inodes) =
            child_env.get_memory_and_wasi_state_and_inodes_mut(&new_store, 0);
        let mut conv_stdio_mode = |mode: WasiStdioMode, fd: WasiFd| -> Result<OptionFd, BusErrno> {
            match mode {
                WasiStdioMode::Piped => {
                    let pipes = WasiBidirectionalPipePair::default();
                    let pipe1 = pipes.recv;
                    let pipe2 = pipes.send;
                    let inode1 = child_state.fs.create_inode_with_default_stat(
                        child_inodes.deref_mut(),
                        Kind::Pipe { pipe: pipe1 },
                        false,
                        "pipe".into(),
                    );
                    let inode2 = child_state.fs.create_inode_with_default_stat(
                        child_inodes.deref_mut(),
                        Kind::Pipe { pipe: pipe2 },
                        false,
                        "pipe".into(),
                    );

                    let rights = super::state::all_socket_rights();
                    let pipe = ctx
                        .data()
                        .state
                        .fs
                        .create_fd(rights, rights, Fdflags::empty(), 0, inode1)
                        .map_err(|_| BusErrno::Internal)?;
                    child_state
                        .fs
                        .create_fd_ext(rights, rights, Fdflags::empty(), 0, inode2, fd)
                        .map_err(|_| BusErrno::Internal)?;

                    trace!(
                        "wasi[{}:{}]::fd_pipe (fd1={}, fd2={})",
                        ctx.data().pid(),
                        ctx.data().tid(),
                        pipe,
                        fd
                    );
                    Ok(OptionFd {
                        tag: OptionTag::Some,
                        fd: pipe,
                    })
                }
                WasiStdioMode::Inherit => Ok(OptionFd {
                    tag: OptionTag::None,
                    fd: u32::MAX,
                }),
                WasiStdioMode::Log | WasiStdioMode::Null | _ => {
                    child_state.fs.close_fd(child_inodes.deref(), fd);
                    Ok(OptionFd {
                        tag: OptionTag::None,
                        fd: u32::MAX,
                    })
                }
            }
        };
        let stdin = conv_stdio_mode(stdin, 0)?;
        let stdout = conv_stdio_mode(stdout, 1)?;
        let stderr = conv_stdio_mode(stderr, 2)?;
        (stdin, stdout, stderr)
    };

    // Create the new process
    let bus = env.runtime.bus();
    let mut process = bus
        .spawn(child_env)
        .spawn(
            Some(&ctx),
            name.as_str(),
            new_store,
            &ctx.data().bin_factory,
        )
        .map_err(vbus_error_into_bus_errno)?;

    // Add the process to the environment state
    let pid = env.process.pid();
    {
        let mut children = ctx.data().process.children.write().unwrap();
        children.push(pid);
    }
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    // Add the process to the environment state
    let pid = env.process.pid();
    {
        let mut children = ctx.data().process.children.write().unwrap();
        children.push(pid);
    }
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    {
        let mut guard = env.process.write();
        guard.bus_processes.insert(pid.into(), Box::new(process));
    };

    let handles = BusHandles {
        bid: pid.raw(),
        stdin,
        stdout,
        stderr,
    };
    Ok((handles, ctx))
}

/// ### `proc_join()`
/// Joins the child process, blocking this one until the other finishes
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
pub fn proc_join<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<Pid, M>,
    exit_code_ptr: WasmPtr<ExitCode, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let pid = wasi_try_mem_ok!(pid_ptr.read(&memory));
    trace!(
        "wasi[{}:{}]::proc_join (pid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        pid
    );

    // If the ID is maximum then it means wait for any of the children
    if pid == u32::MAX {
        let mut process = ctx.data_mut().process.clone();
        let child_exit = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            process.join_any_child().await
        }));
        return match child_exit {
            Some((pid, exit_code)) => {
                trace!(
                    "wasi[{}:{}]::child ({}) exited with {}",
                    ctx.data().pid(),
                    ctx.data().tid(),
                    pid,
                    exit_code
                );
                let env = ctx.data();
                let memory = env.memory_view(&ctx);
                wasi_try_mem_ok!(pid_ptr.write(&memory, pid.raw() as Pid));
                wasi_try_mem_ok!(exit_code_ptr.write(&memory, exit_code));
                Ok(Errno::Success)
            }
            None => {
                trace!(
                    "wasi[{}:{}]::no children",
                    ctx.data().pid(),
                    ctx.data().tid()
                );
                let env = ctx.data();
                let memory = env.memory_view(&ctx);
                wasi_try_mem_ok!(pid_ptr.write(&memory, -1i32 as Pid));
                wasi_try_mem_ok!(exit_code_ptr.write(&memory, Errno::Child as u32));
                Ok(Errno::Child)
            }
        };
    }

    // Otherwise we wait for the specific PID
    let env = ctx.data();
    let pid: WasiProcessId = pid.into();
    let process = env
        .process
        .control_plane()
        .get_process(pid)
        .map(|a| a.clone());
    if let Some(process) = process {
        let exit_code = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
            process.join().await.ok_or(Errno::Child)
        }));

        trace!("child ({}) exited with {}", pid.raw(), exit_code);
        let env = ctx.data();
        let mut children = env.process.children.write().unwrap();
        children.retain(|a| *a != pid);

        let memory = env.memory_view(&ctx);
        wasi_try_mem_ok!(exit_code_ptr.write(&memory, exit_code));
        return Ok(Errno::Success);
    }

    debug!(
        "process already terminated or not registered (pid={})",
        pid.raw()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(exit_code_ptr.write(&memory, Errno::Child as ExitCode));
    Ok(Errno::Child)
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
) -> Result<BusErrno, WasiError> {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus_ok!(&memory, name, name_len) };
    let reuse = reuse == Bool::True;
    debug!(
        "wasi[{}:{}]::bus_open_local (name={}, reuse={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name,
        reuse
    );

    bus_open_internal(ctx, name, reuse, None, None, ret_bid)
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
) -> Result<BusErrno, WasiError> {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus_ok!(&memory, name, name_len) };
    let instance = unsafe { get_input_str_bus_ok!(&memory, instance, instance_len) };
    let token = unsafe { get_input_str_bus_ok!(&memory, token, token_len) };
    let reuse = reuse == Bool::True;
    debug!(
        "wasi::bus_open_remote (name={}, reuse={}, instance={})",
        name, reuse, instance
    );

    bus_open_internal(ctx, name, reuse, Some(instance), Some(token), ret_bid)
}

#[cfg(feature = "os")]
fn bus_open_internal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: String,
    reuse: bool,
    instance: Option<String>,
    token: Option<String>,
    ret_bid: WasmPtr<Bid, M>,
) -> Result<BusErrno, WasiError> {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name: Cow<'static, str> = name.into();

    // Check if it already exists
    if reuse {
        let guard = env.process.read();
        if let Some(bid) = guard.bus_process_reuse.get(&name) {
            if guard.bus_processes.contains_key(bid) {
                wasi_try_mem_bus_ok!(ret_bid.write(&memory, bid.clone().into()));
                return Ok(BusErrno::Success);
            }
        }
    }

    let (handles, ctx) = wasi_try_bus_ok!(proc_spawn_internal(
        ctx,
        name.to_string(),
        None,
        None,
        None,
        WasiStdioMode::Null,
        WasiStdioMode::Null,
        WasiStdioMode::Log
    ));
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let pid: WasiProcessId = handles.bid.into();
    let memory = env.memory_view(&ctx);
    {
        let mut inner = env.process.write();
        inner.bus_process_reuse.insert(name, pid);
    };

    wasi_try_mem_bus_ok!(ret_bid.write(&memory, pid.into()));
    Ok(BusErrno::Success)
}

#[allow(unused_variables)]
#[cfg(not(feature = "os"))]
fn bus_open_internal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: String,
    reuse: bool,
    instance: Option<String>,
    token: Option<String>,
    ret_bid: WasmPtr<Bid, M>,
) -> Result<BusErrno, WasiError> {
    warn!(
        "wasi[{}:{}]::bus_open_internal is not supported on this platform",
        ctx.data().pid(),
        ctx.data().tid()
    );
    Ok(BusErrno::Unsupported)
}

/// Closes a bus process and releases all associated resources
///
/// ## Parameters
///
/// * `bid` - Handle of the bus process handle to be closed
pub fn bus_close(ctx: FunctionEnvMut<'_, WasiEnv>, bid: Bid) -> BusErrno {
    trace!(
        "wasi[{}:{}]::bus_close (bid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        bid
    );
    let pid: WasiProcessId = bid.into();

    let env = ctx.data();
    let mut inner = env.process.write();
    if let Some(process) = inner.bus_processes.remove(&pid) {
        inner.bus_process_reuse.retain(|_, v| *v != pid);
    }

    BusErrno::Success
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    bid: Bid,
    topic_hash: WasmPtr<WasiHash>,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    ret_cid: WasmPtr<Cid, M>,
) -> Result<BusErrno, WasiError> {
    let mut env = ctx.data();
    let bus = env.runtime.bus();
    let topic_hash = {
        let memory = env.memory_view(&ctx);
        wasi_try_mem_bus_ok!(topic_hash.read(&memory))
    };
    trace!("wasi::bus_call (bid={}, buf_len={})", bid, buf_len);

    // Get the process that we'll invoke this call for
    let mut guard = env.process.read();
    let bid: WasiProcessId = bid.into();
    let process = if let Some(process) = { guard.bus_processes.get(&bid) } {
        process
    } else {
        return Ok(BusErrno::Badhandle);
    };

    let format = conv_bus_format_from(format);

    // Check if the process has finished
    if let Some(code) = process.inst.exit_code() {
        debug!("process has already exited (code = {})", code);
        return Ok(BusErrno::Aborted);
    }

    // Invoke the call
    let buf = {
        let memory = env.memory_view(&ctx);
        let buf_slice = wasi_try_mem_bus_ok!(buf.slice(&memory, buf_len));
        wasi_try_mem_bus_ok!(buf_slice.read_to_vec())
    };
    let mut invoked = process.inst.invoke(topic_hash, format, buf);
    drop(process);
    drop(guard);

    // Poll the invocation until it does its thing
    let mut invocation;
    {
        invocation = wasi_try_bus_ok!(__asyncify(&mut ctx, None, async move {
            VirtualBusInvokedWait::new(invoked).await.map_err(|err| {
                debug!(
                    "wasi::bus_call failed (bid={}, buf_len={}) - {}",
                    bid, buf_len, err
                );
                Errno::Io
            })
        })
        .map_err(|_| BusErrno::Invoke));
        env = ctx.data();
    }

    // Record the invocation
    let cid = {
        let mut guard = env.state.bus.protected();
        guard.call_seed += 1;
        let cid = guard.call_seed;
        guard.calls.insert(cid, WasiBusCall { bid, invocation });
        cid
    };

    // Now we wake any BUS pollers so that they can drive forward the
    // call to completion - when they poll the call they will also
    // register a BUS waker
    env.state.bus.poll_wake();

    // Return the CID and success to the caller
    let memory = env.memory_view(&ctx);
    wasi_try_mem_bus_ok!(ret_cid.write(&memory, cid));
    Ok(BusErrno::Success)
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    parent_cid: Cid,
    topic_hash: WasmPtr<WasiHash>,
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    ret_cid: WasmPtr<Cid, M>,
) -> Result<BusErrno, WasiError> {
    let mut env = ctx.data();
    let bus = env.runtime.bus();
    let topic_hash = {
        let memory = env.memory_view(&ctx);
        wasi_try_mem_bus_ok!(topic_hash.read(&memory))
    };
    trace!(
        "wasi::bus_subcall (parent={}, buf_len={})",
        parent_cid,
        buf_len
    );

    let format = conv_bus_format_from(format);
    let buf = {
        let memory = env.memory_view(&ctx);
        let buf_slice = wasi_try_mem_bus_ok!(buf.slice(&memory, buf_len));
        wasi_try_mem_bus_ok!(buf_slice.read_to_vec())
    };

    // Get the parent call that we'll invoke this call for
    let mut guard = env.state.bus.protected();
    if let Some(parent) = guard.calls.get(&parent_cid) {
        let bid = parent.bid.clone();

        // Invoke the sub-call in the existing parent call
        let mut invoked = parent.invocation.invoke(topic_hash, format, buf);
        drop(parent);
        drop(guard);

        // Poll the invocation until it does its thing
        let invocation;
        {
            invocation = wasi_try_bus_ok!(__asyncify(&mut ctx, None, async move {
                VirtualBusInvokedWait::new(invoked).await.map_err(|err| {
                    debug!(
                        "wasi::bus_subcall failed (parent={}, buf_len={}) - {}",
                        parent_cid, buf_len, err
                    );
                    Errno::Io
                })
            })
            .map_err(|_| BusErrno::Invoke));
            env = ctx.data();
        }

        // Add the call and return the ID
        let cid = {
            let mut guard = env.state.bus.protected();
            guard.call_seed += 1;
            let cid = guard.call_seed;
            guard.calls.insert(cid, WasiBusCall { bid, invocation });
            cid
        };

        // Now we wake any BUS pollers so that they can drive forward the
        // call to completion - when they poll the call they will also
        // register a BUS waker
        env.state.bus.poll_wake();

        // Return the CID and success to the caller
        let memory = env.memory_view(&ctx);
        wasi_try_mem_bus_ok!(ret_cid.write(&memory, cid));
        Ok(BusErrno::Success)
    } else {
        Ok(BusErrno::Badhandle)
    }
}

// Function for converting the format
fn conv_bus_format(format: BusDataFormat) -> __wasi_busdataformat_t {
    match format {
        BusDataFormat::Raw => __wasi_busdataformat_t::Raw,
        BusDataFormat::Bincode => __wasi_busdataformat_t::Bincode,
        BusDataFormat::MessagePack => __wasi_busdataformat_t::MessagePack,
        BusDataFormat::Json => __wasi_busdataformat_t::Json,
        BusDataFormat::Yaml => __wasi_busdataformat_t::Yaml,
        BusDataFormat::Xml => __wasi_busdataformat_t::Xml,
        BusDataFormat::Rkyv => __wasi_busdataformat_t::Rkyv,
    }
}

fn conv_bus_format_from(format: __wasi_busdataformat_t) -> BusDataFormat {
    match format {
        __wasi_busdataformat_t::Raw => BusDataFormat::Raw,
        __wasi_busdataformat_t::Bincode => BusDataFormat::Bincode,
        __wasi_busdataformat_t::MessagePack => BusDataFormat::MessagePack,
        __wasi_busdataformat_t::Json => BusDataFormat::Json,
        __wasi_busdataformat_t::Yaml => BusDataFormat::Yaml,
        __wasi_busdataformat_t::Xml => BusDataFormat::Xml,
        __wasi_busdataformat_t::Rkyv => BusDataFormat::Rkyv,
    }
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
#[cfg(feature = "os")]
pub fn bus_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    timeout: Timestamp,
    ref_events: WasmPtr<__wasi_busevent_t, M>,
    maxevents: M::Offset,
    ret_nevents: WasmPtr<M::Offset, M>,
) -> Result<BusErrno, WasiError> {
    use wasmer_wasi_types::wasi::{BusEventType, OptionCid};

    let mut env = ctx.data();
    let bus = env.runtime.bus();
    trace!(
        "wasi[{}:{}]::bus_poll (timeout={})",
        ctx.data().pid(),
        ctx.data().tid(),
        timeout
    );

    // Lets start by processing events for calls that are already running
    let mut nevents = M::ZERO;

    let state = env.state.clone();
    let start = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
    loop {
        // The waker will wake this thread should any work arrive
        // or need further processing (i.e. async operation)
        let waker = state.bus.get_poll_waker();
        let mut cx = Context::from_waker(&waker);

        // Check if any of the processes have closed
        let mut exited_bids = HashSet::new();
        {
            let mut inner = env.process.write();
            for (pid, process) in inner.bus_processes.iter_mut() {
                let pinned_process = Pin::new(process.inst.as_mut());
                if pinned_process.poll_finished(&mut cx) == Poll::Ready(()) {
                    exited_bids.insert(*pid);
                }
            }
            for pid in exited_bids.iter() {
                inner.bus_processes.remove(pid);
            }
        }

        {
            // The waker will trigger the reactors when work arrives from the BUS
            let mut guard = env.state.bus.protected();

            // Function that hashes the topic using SHA256
            let hash_topic = |topic: Cow<'static, str>| -> WasiHash {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&topic.bytes().collect::<Vec<_>>());
                let hash: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();
                u128::from_le_bytes(hash)
            };

            // Function that turns a buffer into a readable file handle
            let buf_to_fd = {
                let state = env.state.clone();
                let inodes = state.inodes.clone();
                move |data: Vec<u8>| -> Result<WasiFd, BusErrno> {
                    let mut inodes = inodes.write().unwrap();
                    let inode = state.fs.create_inode_with_default_stat(
                        inodes.deref_mut(),
                        Kind::Buffer { buffer: data },
                        false,
                        "bus".into(),
                    );
                    let rights = super::state::bus_read_rights();
                    state
                        .fs
                        .create_fd(rights, rights, Fdflags::empty(), 0, inode)
                        .map_err(|err| {
                            debug!(
                                "failed to create file descriptor for BUS event buffer - {}",
                                err
                            );
                            BusErrno::Alloc
                        })
                }
            };

            // Grab all the events we can from all the existing calls up to the limit of
            // maximum events that the user requested
            if nevents < maxevents {
                let mut drop_calls = Vec::new();
                let mut call_seed = guard.call_seed;
                for (key, call) in guard.calls.iter_mut() {
                    let cid: Cid = (*key).into();

                    if nevents >= maxevents {
                        break;
                    }

                    // If the process that is hosting the call is finished then so is the call
                    if exited_bids.contains(&call.bid) {
                        drop_calls.push(*key);
                        trace!(
                            "wasi[{}:{}]::bus_poll (aborted, cid={})",
                            ctx.data().pid(),
                            ctx.data().tid(),
                            cid
                        );
                        let evt = unsafe {
                            std::mem::transmute(__wasi_busevent_t2 {
                                tag: BusEventType::Fault,
                                u: __wasi_busevent_u {
                                    fault: __wasi_busevent_fault_t {
                                        cid,
                                        err: BusErrno::Aborted,
                                    },
                                },
                            })
                        };

                        let nevents64: u64 =
                            wasi_try_bus_ok!(nevents.try_into().map_err(|_| BusErrno::Internal));
                        let memory = env.memory_view(&ctx);
                        let events = wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                        wasi_try_mem_bus_ok!(events.write(nevents64, evt));

                        nevents += M::ONE;
                        continue;
                    }

                    // Otherwise lets poll for events
                    while nevents < maxevents {
                        let mut finished = false;
                        let call = Pin::new(call.invocation.as_mut());
                        match call.poll_event(&mut cx) {
                            Poll::Ready(evt) => {
                                let evt = match evt {
                                    BusInvocationEvent::Callback {
                                        topic_hash,
                                        format,
                                        data,
                                    } => {
                                        let sub_cid = {
                                            call_seed += 1;
                                            call_seed
                                        };

                                        trace!("wasi[{}:{}]::bus_poll (callback, parent={}, cid={}, topic={})", ctx.data().pid(), ctx.data().tid(), cid, sub_cid, topic_hash);
                                        __wasi_busevent_t2 {
                                            tag: BusEventType::Call,
                                            u: __wasi_busevent_u {
                                                call: __wasi_busevent_call_t {
                                                    parent: OptionCid {
                                                        tag: OptionTag::Some,
                                                        cid,
                                                    },
                                                    cid: sub_cid,
                                                    format: conv_bus_format(format),
                                                    topic_hash,
                                                    fd: wasi_try_bus_ok!(buf_to_fd(data)),
                                                },
                                            },
                                        }
                                    }
                                    BusInvocationEvent::Response { format, data } => {
                                        drop_calls.push(*key);
                                        finished = true;

                                        trace!(
                                            "wasi[{}:{}]::bus_poll (response, cid={}, len={})",
                                            ctx.data().pid(),
                                            ctx.data().tid(),
                                            cid,
                                            data.len()
                                        );
                                        __wasi_busevent_t2 {
                                            tag: BusEventType::Result,
                                            u: __wasi_busevent_u {
                                                result: __wasi_busevent_result_t {
                                                    format: conv_bus_format(format),
                                                    cid,
                                                    fd: wasi_try_bus_ok!(buf_to_fd(data)),
                                                },
                                            },
                                        }
                                    }
                                    BusInvocationEvent::Fault { fault } => {
                                        drop_calls.push(*key);
                                        finished = true;

                                        trace!(
                                            "wasi[{}:{}]::bus_poll (fault, cid={}, err={})",
                                            ctx.data().pid(),
                                            ctx.data().tid(),
                                            cid,
                                            fault
                                        );
                                        __wasi_busevent_t2 {
                                            tag: BusEventType::Fault,
                                            u: __wasi_busevent_u {
                                                fault: __wasi_busevent_fault_t {
                                                    cid,
                                                    err: vbus_error_into_bus_errno(fault),
                                                },
                                            },
                                        }
                                    }
                                };
                                let evt = unsafe { std::mem::transmute(evt) };

                                let memory = env.memory_view(&ctx);
                                let events =
                                    wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                                let nevents64: u64 = wasi_try_bus_ok!(nevents
                                    .try_into()
                                    .map_err(|_| BusErrno::Internal));
                                wasi_try_mem_bus_ok!(events.write(nevents64, evt));

                                nevents += M::ONE;

                                if finished {
                                    break;
                                }
                            }
                            Poll::Pending => {
                                break;
                            }
                        }
                    }
                }
                guard.call_seed = call_seed;

                // Drop any calls that are no longer in scope
                if drop_calls.is_empty() == false {
                    for key in drop_calls {
                        guard.calls.remove(&key);
                    }
                }
            }

            if nevents < maxevents {
                let mut call_seed = guard.call_seed;
                let mut to_add = Vec::new();
                for (key, call) in guard.called.iter_mut() {
                    let cid: Cid = (*key).into();
                    while nevents < maxevents {
                        let call = Pin::new(call.deref_mut());
                        match call.poll(&mut cx) {
                            Poll::Ready(event) => {
                                // Register the call
                                let sub_cid = {
                                    call_seed += 1;
                                    to_add.push((call_seed, event.called));
                                    call_seed
                                };

                                let event = __wasi_busevent_t2 {
                                    tag: BusEventType::Call,
                                    u: __wasi_busevent_u {
                                        call: __wasi_busevent_call_t {
                                            parent: OptionCid {
                                                tag: OptionTag::Some,
                                                cid,
                                            },
                                            cid: sub_cid,
                                            format: conv_bus_format(event.format),
                                            topic_hash: event.topic_hash,
                                            fd: wasi_try_bus_ok!(buf_to_fd(event.data)),
                                        },
                                    },
                                };
                                let event = unsafe { std::mem::transmute(event) };

                                let memory = env.memory_view(&ctx);
                                let events =
                                    wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                                let nevents64: u64 = wasi_try_bus_ok!(nevents
                                    .try_into()
                                    .map_err(|_| BusErrno::Internal));
                                wasi_try_mem_bus_ok!(events.write(nevents64, event));
                                nevents += M::ONE;
                            }
                            Poll::Pending => {
                                break;
                            }
                        };
                    }
                    if nevents >= maxevents {
                        break;
                    }
                }

                guard.call_seed = call_seed;
                for (cid, called) in to_add {
                    guard.called.insert(cid, called);
                }
            }

            while nevents < maxevents {
                // Check the listener (if none exists then one is created)
                let event = {
                    let bus = env.runtime.bus();
                    let listener =
                        wasi_try_bus_ok!(bus.listen().map_err(vbus_error_into_bus_errno));
                    let listener = Pin::new(listener.deref());
                    listener.poll(&mut cx)
                };

                // Process the event returned by the listener or exit the poll loop
                let event = match event {
                    Poll::Ready(event) => {
                        // Register the call
                        let sub_cid = {
                            guard.call_seed += 1;
                            let cid = guard.call_seed;
                            guard.called.insert(cid, event.called);
                            cid
                        };

                        __wasi_busevent_t2 {
                            tag: BusEventType::Call,
                            u: __wasi_busevent_u {
                                call: __wasi_busevent_call_t {
                                    parent: OptionCid {
                                        tag: OptionTag::None,
                                        cid: 0,
                                    },
                                    cid: sub_cid,
                                    format: conv_bus_format(event.format),
                                    topic_hash: event.topic_hash,
                                    fd: wasi_try_bus_ok!(buf_to_fd(event.data)),
                                },
                            },
                        }
                    }
                    Poll::Pending => {
                        break;
                    }
                };
                let event = unsafe { std::mem::transmute(event) };

                let memory = env.memory_view(&ctx);
                let events = wasi_try_mem_bus_ok!(ref_events.slice(&memory, maxevents));
                let nevents64: u64 =
                    wasi_try_bus_ok!(nevents.try_into().map_err(|_| BusErrno::Internal));
                wasi_try_mem_bus_ok!(events.write(nevents64, event));
                nevents += M::ONE;
            }
        }

        // If we still have no events
        if nevents >= M::ONE {
            break;
        }

        // Every 100 milliseconds we check if the thread needs to terminate (via `env.yield_now`)
        // otherwise the loop will break if the BUS futex is triggered or a timeout is reached
        loop {
            // Check for timeout (zero will mean the loop will not wait)
            let now =
                platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
            let delta = now.checked_sub(start).unwrap_or(0) as Timestamp;
            if delta >= timeout {
                trace!(
                    "wasi[{}:{}]::bus_poll (timeout)",
                    ctx.data().pid(),
                    ctx.data().tid()
                );
                let memory = env.memory_view(&ctx);
                wasi_try_mem_bus_ok!(ret_nevents.write(&memory, nevents));
                return Ok(BusErrno::Success);
            }

            let _ = ctx.data().clone().process_signals_and_exit(&mut ctx)?;
            env = ctx.data();

            let remaining = timeout.checked_sub(delta).unwrap_or(0);
            let interval = Duration::from_nanos(remaining)
                .min(Duration::from_millis(5)) // we don't want the CPU burning
                .max(Duration::from_millis(100)); // 100 milliseconds to kill worker threads seems acceptable
            if state.bus.poll_wait(interval) == true {
                break;
            }
        }
    }
    if nevents > M::ZERO {
        trace!(
            "wasi[{}:{}]::bus_poll (return nevents={})",
            ctx.data().pid(),
            ctx.data().tid(),
            nevents
        );
    } else {
        trace!(
            "wasi[{}:{}]::bus_poll (idle - no events)",
            ctx.data().pid(),
            ctx.data().tid()
        );
    }

    let memory = env.memory_view(&ctx);
    wasi_try_mem_bus_ok!(ret_nevents.write(&memory, nevents));
    Ok(BusErrno::Success)
}

#[cfg(not(feature = "os"))]
pub fn bus_poll<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    timeout: Timestamp,
    events: WasmPtr<__wasi_busevent_t, M>,
    maxevents: M::Offset,
    ret_nevents: WasmPtr<M::Offset, M>,
) -> Result<BusErrno, WasiError> {
    trace!(
        "wasi[{}:{}]::bus_poll (timeout={}) is not supported without 'os' feature",
        ctx.data().pid(),
        ctx.data().tid(),
        timeout
    );
    Ok(BusErrno::Unsupported)
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
    format: __wasi_busdataformat_t,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
) -> BusErrno {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let bus = env.runtime.bus();
    trace!(
        "wasi::call_reply (cid={}, format={}, data_len={})",
        cid,
        format,
        buf_len
    );
    let buf_slice = wasi_try_mem_bus!(buf.slice(&memory, buf_len));
    let buf = wasi_try_mem_bus!(buf_slice.read_to_vec());

    let mut guard = env.state.bus.protected();
    if let Some(call) = guard.called.remove(&cid) {
        drop(guard);

        let format = conv_bus_format_from(format);
        call.reply(format, buf);
        BusErrno::Success
    } else {
        BusErrno::Badhandle
    }
}

/// Causes a fault on a particular call that was made
/// to this process from another process; where 'bid'
/// is the callering process context.
///
/// ## Parameters
///
/// * `cid` - Handle of the call to raise a fault on
/// * `fault` - Fault to be raised on the bus
pub fn call_fault(ctx: FunctionEnvMut<'_, WasiEnv>, cid: Cid, fault: BusErrno) {
    let env = ctx.data();
    let bus = env.runtime.bus();
    debug!(
        "wasi[{}:{}]::call_fault (cid={}, fault={})",
        ctx.data().pid(),
        ctx.data().tid(),
        cid,
        fault
    );

    let mut guard = env.state.bus.protected();
    guard.calls.remove(&cid);

    if let Some(call) = guard.called.remove(&cid) {
        drop(guard);
        call.fault(bus_errno_into_vbus_error(fault));
    }
}

/// Closes a bus call based on its bus call handle
///
/// ## Parameters
///
/// * `cid` - Handle of the bus call handle to be dropped
pub fn call_close(ctx: FunctionEnvMut<'_, WasiEnv>, cid: Cid) {
    let env = ctx.data();
    let bus = env.runtime.bus();
    trace!(
        "wasi[{}:{}]::call_close (cid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        cid
    );

    let mut guard = env.state.bus.protected();
    guard.calls.remove(&cid);
    guard.called.remove(&cid);
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    url: WasmPtr<u8, M>,
    url_len: M::Offset,
    ret_sock: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::ws_connect",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let mut env = ctx.data();
    let memory = env.memory_view(&ctx);
    let url = unsafe { get_input_str!(&memory, url, url_len) };

    let net = env.net();
    let tasks = env.tasks.clone();
    let socket = wasi_try!(__asyncify(&mut ctx, None, async move {
        net.ws_connect(url.as_str())
            .await
            .map_err(net_error_into_wasi_err)
    }));
    env = ctx.data();

    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::WebSocket(socket)),
    };

    let inode =
        state
            .fs
            .create_inode_with_default_stat(inodes.deref_mut(), kind, false, "socket".into());
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    url: WasmPtr<u8, M>,
    url_len: M::Offset,
    method: WasmPtr<u8, M>,
    method_len: M::Offset,
    headers: WasmPtr<u8, M>,
    headers_len: M::Offset,
    gzip: Bool,
    ret_handles: WasmPtr<HttpHandles, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::http_request",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let mut env = ctx.data();
    let memory = env.memory_view(&ctx);
    let url = unsafe { get_input_str!(&memory, url, url_len) };
    let method = unsafe { get_input_str!(&memory, method, method_len) };
    let headers = unsafe { get_input_str!(&memory, headers, headers_len) };

    let gzip = match gzip {
        Bool::False => false,
        Bool::True => true,
        _ => return Errno::Inval,
    };

    let net = env.net();
    let tasks = env.tasks.clone();
    let socket = wasi_try!(__asyncify(&mut ctx, None, async move {
        net.http_request(url.as_str(), method.as_str(), headers.as_str(), gzip)
            .await
            .map_err(net_error_into_wasi_err)
    }));
    env = ctx.data();

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
        "http_request".to_string().into(),
    );
    let inode_res = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_res,
        false,
        "http_response".to_string().into(),
    );
    let inode_hdr = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_hdr,
        false,
        "http_headers".to_string().into(),
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ref_status: WasmPtr<HttpStatus, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::http_status",
        ctx.data().pid(),
        ctx.data().tid()
    );

    let mut env = ctx.data();

    let http_status = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.http_status() }
    ));
    env = ctx.data();

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

    let memory = env.memory_view(&ctx);
    let ref_status = ref_status.deref(&memory);
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
    debug!(
        "wasi[{}:{}]::port_bridge",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::port_unbridge",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    wasi_try!(env.net().unbridge().map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_dhcp_acquire()`
/// Acquires a set of IP addresses using DHCP
pub fn port_dhcp_acquire(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!(
        "wasi[{}:{}]::port_dhcp_acquire",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let net = env.net();
    let tasks = env.tasks.clone();
    wasi_try!(__asyncify(&mut ctx, None, async move {
        net.dhcp_acquire().await.map_err(net_error_into_wasi_err)
    }));
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
    debug!(
        "wasi[{}:{}]::port_addr_add",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::port_addr_remove",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try!(super::state::read_ip(&memory, ip));
    wasi_try!(env.net().ip_remove(ip).map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_addr_clear()`
/// Clears all the addresses on the local port
pub fn port_addr_clear(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!(
        "wasi[{}:{}]::port_addr_clear",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!("wasi[{}:{}]::port_mac", ctx.data().pid(), ctx.data().tid());
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
    debug!(
        "wasi[{}:{}]::port_addr_list",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::port_gateway_set",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::port_route_add",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::port_route_remove",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let ip = wasi_try!(super::state::read_ip(&memory, ip));
    wasi_try!(env.net().route_remove(ip).map_err(net_error_into_wasi_err));
    Errno::Success
}

/// ### `port_route_clear()`
/// Clears all the routes in the local port
pub fn port_route_clear(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!(
        "wasi[{}:{}]::port_route_clear",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
    debug!(
        "wasi[{}:{}]::port_route_list",
        ctx.data().pid(),
        ctx.data().tid()
    );
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
pub fn sock_shutdown(mut ctx: FunctionEnvMut<'_, WasiEnv>, sock: WasiFd, how: SdFlags) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_shutdown (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let both = __WASI_SHUT_RD | __WASI_SHUT_WR;
    let how = match how {
        __WASI_SHUT_RD => std::net::Shutdown::Read,
        __WASI_SHUT_WR => std::net::Shutdown::Write,
        a if a == both => std::net::Shutdown::Both,
        _ => return Errno::Inval,
    };

    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_SHUTDOWN,
        move |mut socket| async move { socket.shutdown(how).await }
    ));

    Errno::Success
}

/// ### `sock_status()`
/// Returns the current status of a socket
pub fn sock_status<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ret_status: WasmPtr<Sockstatus, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_status (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let status = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.status() }
    ));

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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ret_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_addr_local (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let addr = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.addr_local() }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_addr_peer (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let addr = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.addr_peer() }
    ));

    let env = ctx.data();
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
    debug!("wasi[{}:{}]::sock_open", ctx.data().pid(), ctx.data().tid());

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
                nonblocking: false,
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
        "socket".to_string().into(),
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    flag: Bool,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_set_opt_flag(fd={}, ty={}, flag={:?})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt,
        flag
    );

    let flag = match flag {
        Bool::False => false,
        Bool::True => true,
        _ => return Errno::Inval,
    };

    let option: super::state::WasiSocketOption = opt.into();
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |mut socket| async move { socket.set_opt_flag(option, flag) }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_flag: WasmPtr<Bool, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_get_opt_flag(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );

    let option: super::state::WasiSocketOption = opt.into();
    let flag = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.get_opt_flag(option) }
    ));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    time: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_set_opt_time(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );

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
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.set_opt_time(ty, time) }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_time: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_get_opt_time(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );

    let ty = match opt {
        Sockoption::RecvTimeout => wasmer_vnet::TimeType::ReadTimeout,
        Sockoption::SendTimeout => wasmer_vnet::TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => wasmer_vnet::TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => wasmer_vnet::TimeType::AcceptTimeout,
        Sockoption::Linger => wasmer_vnet::TimeType::Linger,
        _ => return Errno::Inval,
    };

    let time = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.opt_time(ty) }
    ));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);

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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    size: Filesize,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_set_opt_size(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );

    let ty = match opt {
        Sockoption::RecvTimeout => wasmer_vnet::TimeType::ReadTimeout,
        Sockoption::SendTimeout => wasmer_vnet::TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => wasmer_vnet::TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => wasmer_vnet::TimeType::AcceptTimeout,
        Sockoption::Linger => wasmer_vnet::TimeType::Linger,
        _ => return Errno::Inval,
    };

    let option: super::state::WasiSocketOption = opt.into();
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |mut socket| async move {
            match opt {
                Sockoption::RecvBufSize => socket.set_recv_buf_size(size as usize),
                Sockoption::SendBufSize => socket.set_send_buf_size(size as usize),
                Sockoption::Ttl => socket.set_ttl(size as u32),
                Sockoption::MulticastTtlV4 => socket.set_multicast_ttl_v4(size as u32),
                _ => Err(Errno::Inval),
            }
        }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_size: WasmPtr<Filesize, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_get_opt_size(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );
    let size = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move {
            match opt {
                Sockoption::RecvBufSize => socket.recv_buf_size().map(|a| a as Filesize),
                Sockoption::SendBufSize => socket.send_buf_size().map(|a| a as Filesize),
                Sockoption::Ttl => socket.ttl().map(|a| a as Filesize),
                Sockoption::MulticastTtlV4 => socket.multicast_ttl_v4().map(|a| a as Filesize),
                _ => Err(Errno::Inval),
            }
        }
    ));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, M>,
    iface: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_join_multicast_v4 (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v4(&memory, multiaddr));
    let iface = wasi_try!(super::state::read_ip_v4(&memory, iface));
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.join_multicast_v4(multiaddr, iface).await }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip4_t, M>,
    iface: WasmPtr<__wasi_addr_ip4_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_leave_multicast_v4 (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v4(&memory, multiaddr));
    let iface = wasi_try!(super::state::read_ip_v4(&memory, iface));
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.leave_multicast_v4(multiaddr, iface).await }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, M>,
    iface: u32,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_join_multicast_v6 (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v6(&memory, multiaddr));
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.join_multicast_v6(multiaddr, iface).await }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    multiaddr: WasmPtr<__wasi_addr_ip6_t, M>,
    iface: u32,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_leave_multicast_v6 (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let multiaddr = wasi_try!(super::state::read_ip_v6(&memory, multiaddr));
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        move |mut socket| async move { socket.leave_multicast_v6(multiaddr, iface).await }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_bind (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let addr = wasi_try!(super::state::read_ip_port(&memory, addr));
    let addr = SocketAddr::new(addr.0, addr.1);
    let net = env.net();
    wasi_try!(__sock_upgrade(
        &mut ctx,
        sock,
        Rights::SOCK_BIND,
        move |socket| async move { socket.bind(net, addr).await }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    backlog: M::Offset,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_listen (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let net = env.net();
    let backlog: usize = wasi_try!(backlog.try_into().map_err(|_| Errno::Inval));
    wasi_try!(__sock_upgrade(
        &mut ctx,
        sock,
        Rights::SOCK_LISTEN,
        move |socket| async move { socket.listen(net, backlog).await }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    fd_flags: Fdflags,
    ro_fd: WasmPtr<WasiFd, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_accept (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let (child, addr) = wasi_try_ok!(__sock_actor(
        &mut ctx,
        sock,
        Rights::SOCK_ACCEPT,
        move |socket| async move { socket.accept(fd_flags).await }
    ));

    let env = ctx.data();
    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::TcpStream(child)),
    };
    let inode =
        state
            .fs
            .create_inode_with_default_stat(inodes.deref_mut(), kind, false, "socket".into());

    let rights = Rights::all_socket();
    let fd = wasi_try_ok!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    debug!(
        "wasi[{}:{}]::sock_accept (ret=ESUCCESS, peer={})",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );

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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_connect (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let env = ctx.data();
    let net = env.net();
    let memory = env.memory_view(&ctx);
    let addr = wasi_try!(super::state::read_ip_port(&memory, addr));
    let addr = SocketAddr::new(addr.0, addr.1);
    wasi_try!(__sock_upgrade(
        &mut ctx,
        sock,
        Rights::SOCK_CONNECT,
        move |mut socket| async move { socket.connect(net, addr).await }
    ));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    _ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_recv (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );
    let mut env = ctx.data();

    let max_size = {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));
        let mut max_size = 0usize;
        for iovs in iovs_arr.iter() {
            let iovs = wasi_try_mem_ok!(iovs.read());
            let buf_len: usize = wasi_try_ok!(iovs.buf_len.try_into().map_err(|_| Errno::Overflow));
            max_size += buf_len;
        }
        max_size
    };

    let data = wasi_try_ok!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_RECV,
        move |socket| async move { socket.recv(max_size).await }
    ));
    env = ctx.data();

    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));

    let data_len = data.len();
    let mut reader = &data[..];
    let bytes_read = wasi_try_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| data_len));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    _ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_recv_from (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    let mut env = ctx.data();

    let max_size = {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));
        let mut max_size = 0usize;
        for iovs in iovs_arr.iter() {
            let iovs = wasi_try_mem_ok!(iovs.read());
            let buf_len: usize = wasi_try_ok!(iovs.buf_len.try_into().map_err(|_| Errno::Overflow));
            max_size += buf_len;
        }
        max_size
    };

    let (data, peer) = wasi_try_ok!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_RECV_FROM,
        move |socket| async move { socket.recv_from(max_size).await }
    ));
    env = ctx.data();

    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));
    wasi_try_ok!(write_ip_port(&memory, ro_addr, peer.ip(), peer.port()));

    let data_len = data.len();
    let mut reader = &data[..];
    let bytes_read = wasi_try_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| data_len));
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    _si_flags: SiFlags,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_send (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );
    let mut env = ctx.data();
    let runtime = env.runtime.clone();

    let buf_len: M::Offset = {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));
        iovs_arr
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum()
    };
    let buf_len: usize = wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
    let mut buf = Vec::with_capacity(buf_len);
    {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));
        wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));
    }

    let bytes_written = wasi_try_ok!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_SEND,
        move |socket| async move { socket.send(buf).await }
    ));
    env = ctx.data();

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    let memory = env.memory_view(&ctx);
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    _si_flags: SiFlags,
    addr: WasmPtr<__wasi_addr_port_t, M>,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_send_to (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );
    let mut env = ctx.data();

    let buf_len: M::Offset = {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));
        iovs_arr
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum()
    };
    let buf_len: usize = wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
    let mut buf = Vec::with_capacity(buf_len);
    {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));
        wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));
    }

    let (addr_ip, addr_port) = {
        let memory = env.memory_view(&ctx);
        wasi_try_ok!(read_ip_port(&memory, addr))
    };
    let addr = SocketAddr::new(addr_ip, addr_port);

    let bytes_written = wasi_try_ok!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_SEND_TO,
        move |socket| async move { socket.send_to::<M>(buf, addr).await }
    ));
    env = ctx.data();

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    let memory = env.memory_view(&ctx);
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
pub fn sock_send_file<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    in_fd: WasiFd,
    offset: Filesize,
    mut count: Filesize,
    ret_sent: WasmPtr<Filesize, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::send_file (fd={}, file_fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        in_fd
    );
    let mut env = ctx.data();
    let net = env.net();
    let tasks = env.tasks.clone();
    let state = env.state.clone();

    // Set the offset of the file
    {
        let mut fd_map = state.fs.fd_map.write().unwrap();
        let fd_entry = wasi_try_ok!(fd_map.get_mut(&in_fd).ok_or(Errno::Badf));
        fd_entry.offset.store(offset as u64, Ordering::Release);
    }

    // Enter a loop that will process all the data
    let mut total_written: Filesize = 0;
    while (count > 0) {
        let mut buf = [0; 4096];
        let sub_count = count.min(4096);
        count -= sub_count;

        let fd_entry = wasi_try_ok!(state.fs.get_fd(in_fd));
        let bytes_read = {
            let (memory, _, mut inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
            match in_fd {
                __WASI_STDIN_FILENO => {
                    let mut stdin = wasi_try_ok!(inodes
                        .stdin_mut(&state.fs.fd_map)
                        .map_err(fs_error_into_wasi_err));
                    wasi_try_ok!(stdin.read(&mut buf).map_err(map_io_err))
                }
                __WASI_STDOUT_FILENO | __WASI_STDERR_FILENO => return Ok(Errno::Inval),
                _ => {
                    if !fd_entry.rights.contains(Rights::FD_READ) {
                        // TODO: figure out the error to return when lacking rights
                        return Ok(Errno::Access);
                    }

                    let offset = fd_entry.offset.load(Ordering::Acquire) as usize;
                    let inode_idx = fd_entry.inode;
                    let inode = &inodes.arena[inode_idx];

                    let bytes_read = {
                        let mut guard = inode.write();
                        match guard.deref_mut() {
                            Kind::File { handle, .. } => {
                                if let Some(handle) = handle {
                                    let mut handle = handle.write().unwrap();
                                    wasi_try_ok!(handle
                                        .seek(std::io::SeekFrom::Start(offset as u64))
                                        .map_err(map_io_err));
                                    wasi_try_ok!(handle.read(&mut buf).map_err(map_io_err))
                                } else {
                                    return Ok(Errno::Inval);
                                }
                            }
                            Kind::Socket { socket } => {
                                let socket = socket.clone();
                                let tasks = tasks.clone();
                                let max_size = buf.len();
                                drop(guard);
                                drop(inodes);
                                let data = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                                    socket.recv(max_size).await
                                }));
                                env = ctx.data();

                                buf.copy_from_slice(&data[..]);
                                data.len()
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
                    fd_entry
                        .offset
                        .fetch_add(bytes_read as u64, Ordering::AcqRel);

                    bytes_read
                }
            }
        };

        // Write it down to the socket
        let buf = (&buf[..]).to_vec();
        let bytes_written = wasi_try_ok!(__sock_actor_mut(
            &mut ctx,
            sock,
            Rights::SOCK_SEND,
            move |socket| async move { socket.send(buf).await }
        ));
        env = ctx.data();

        total_written += bytes_written as u64;
    }

    let memory = env.memory_view(&ctx);
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    host: WasmPtr<u8, M>,
    host_len: M::Offset,
    port: u16,
    addrs: WasmPtr<__wasi_addr_t, M>,
    naddrs: M::Offset,
    ret_naddrs: WasmPtr<M::Offset, M>,
) -> Errno {
    let naddrs: usize = wasi_try!(naddrs.try_into().map_err(|_| Errno::Inval));
    let mut env = ctx.data();
    let host_str = {
        let memory = env.memory_view(&ctx);
        unsafe { get_input_str!(&memory, host, host_len) }
    };

    debug!(
        "wasi[{}:{}]::resolve (host={})",
        ctx.data().pid(),
        ctx.data().tid(),
        host_str
    );

    let port = if port > 0 { Some(port) } else { None };

    let net = env.net();
    let tasks = env.tasks.clone();
    let found_ips = wasi_try!(__asyncify(&mut ctx, None, async move {
        net.resolve(host_str.as_str(), port, None)
            .await
            .map_err(net_error_into_wasi_err)
    }));
    env = ctx.data();

    let mut idx = 0;
    let memory = env.memory_view(&ctx);
    let addrs = wasi_try_mem!(addrs.slice(&memory, wasi_try!(to_offset::<M>(naddrs))));
    for found_ip in found_ips.iter().take(naddrs) {
        super::state::write_ip(&memory, addrs.index(idx).as_ptr::<M>(), *found_ip);
        idx += 1;
    }

    let idx: M::Offset = wasi_try!(idx.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem!(ret_naddrs.write(&memory, idx));

    Errno::Success
}
