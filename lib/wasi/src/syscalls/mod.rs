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

pub mod wasi;
pub mod wasix;

pub use wasi::*;
pub use wasix::*;

pub mod legacy;

pub(crate) use self::types::{
    wasi::{
        Addressfamily, Advice, Bid, BusDataFormat, BusErrno, BusHandles, Cid, Clockid, Dircookie,
        Dirent, Errno, Event, EventFdReadwrite, Eventrwflags, Eventtype, ExitCode, Fd as WasiFd,
        Fdflags, Fdstat, Filesize, Filestat, Filetype, Fstflags, Linkcount, Longsize, OptionFd,
        Pid, Prestat, Rights, Snapshot0Clockid, Sockoption, Sockstatus, Socktype, StackSnapshot,
        StdioMode as WasiStdioMode, Streamsecurity, Subscription, SubscriptionFsReadwrite, Tid,
        Timestamp, TlKey, TlUser, TlVal, Tty, WasiHash, Whence,
    },
    *,
};

pub(crate) use crate::{
    bin_factory::spawn_exec_module,
    current_caller_id, import_object_for_all_wasi_versions, mem_error_to_wasi,
    net::{
        read_ip_port,
        socket::{InodeHttpSocketType, InodeSocket, InodeSocketKind},
        write_ip_port,
    },
    runtime::SpawnType,
    state::{
        self, bus_errno_into_vbus_error, fs_error_into_wasi_err, iterate_poll_events,
        net_error_into_wasi_err, vbus_error_into_bus_errno, Inode, PollEvent, PollEventBuilder,
        WasiBusCall, WasiDummyWaker, WasiFutex, WasiParkingLot, WasiState, WasiThreadContext,
    },
    utils::{self, map_io_err},
    VirtualTaskManager, WasiEnv, WasiEnvInner, WasiError, WasiFunctionEnv,
    WasiRuntimeImplementation, WasiVFork, DEFAULT_STACK_SIZE,
};
pub(crate) use bytes::{Bytes, BytesMut};
pub(crate) use cooked_waker::IntoWaker;
pub(crate) use sha2::Sha256;
pub(crate) use std::borrow::{Borrow, Cow};
pub(crate) use std::cell::RefCell;
pub(crate) use std::collections::hash_map::Entry;
pub(crate) use std::collections::{HashMap, HashSet};
pub(crate) use std::convert::{Infallible, TryInto};
pub(crate) use std::io::{self, Read, Seek, Write};
pub(crate) use std::mem::transmute;
pub(crate) use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
pub(crate) use std::num::NonZeroU64;
pub(crate) use std::ops::{Deref, DerefMut};
pub(crate) use std::path::Path;
pub(crate) use std::pin::Pin;
pub(crate) use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
pub(crate) use std::sync::{atomic::Ordering, Mutex};
pub(crate) use std::sync::{mpsc, Arc, Condvar};
pub(crate) use std::task::{Context, Poll};
pub(crate) use std::thread::LocalKey;
pub(crate) use std::time::Duration;
pub(crate) use tracing::{debug, error, trace, warn};
pub(crate) use wasmer::vm::VMMemory;
pub(crate) use wasmer::{
    AsStoreMut, AsStoreRef, Extern, Function, FunctionEnv, FunctionEnvMut, Global, Instance,
    Memory, Memory32, Memory64, MemoryAccessError, MemoryError, MemorySize, MemoryView, Module,
    OnCalledAction, Pages, RuntimeError, Store, StoreSnapshot, TypedFunction, Value, WasmPtr,
    WasmSlice,
};
pub(crate) use wasmer_vbus::{
    BusInvocationEvent, BusSpawnedProcess, SignalHandlerAbi, SpawnOptionsConfig, StdioMode,
    VirtualBusError, VirtualBusInvokedWait,
};
pub(crate) use wasmer_vfs::{
    AsyncSeekExt, AsyncWriteExt, FileSystem, FsError, VirtualFile, WasiBidirectionalPipePair,
};
pub(crate) use wasmer_vnet::{SocketHttpRequest, StreamSecurity};
pub(crate) use wasmer_wasi_types::{asyncify::__wasi_asyncify_t, wasi::EventUnion};

#[cfg(any(
    target_os = "freebsd",
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple"
))]
pub use unix::*;

#[cfg(any(target_os = "windows"))]
pub use windows::*;

pub(crate) use crate::os::task::process::WasiProcessId;
pub(crate) use crate::os::task::process::WasiProcessWait;
pub(crate) use crate::os::task::thread::WasiThread;
pub(crate) use crate::os::task::thread::WasiThreadId;
#[cfg(any(target_family = "wasm"))]
pub use wasm::*;

use crate::fs::{virtual_file_type_to_wasi_file_type, Fd, InodeVal, Kind, MAX_SYMLINKS};

pub(crate) fn to_offset<M: MemorySize>(offset: usize) -> Result<M::Offset, Errno> {
    let ret: M::Offset = offset.try_into().map_err(|_| Errno::Inval)?;
    Ok(ret)
}

pub(crate) fn from_offset<M: MemorySize>(offset: M::Offset) -> Result<usize, Errno> {
    let ret: usize = offset.try_into().map_err(|_| Errno::Inval)?;
    Ok(ret)
}

pub(crate) fn write_bytes_inner<T: Write, M: MemorySize>(
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

pub async fn stderr_write(ctx: &FunctionEnvMut<'_, WasiEnv>, buf: &[u8]) -> Result<(), Errno> {
    let env = ctx.data();
    let (memory, state, inodes) = env.get_memory_and_wasi_state_and_inodes_mut(ctx, 0);

    let mut stderr = inodes
        .stderr_mut(&state.fs.fd_map)
        .map_err(fs_error_into_wasi_err)?;

    stderr.write_all(buf).await.map_err(map_io_err)
}

/// Asyncify takes the current thread and blocks on the async runtime associated with it
/// thus allowed for asynchronous operations to execute. It has built in functionality
/// to (optionally) timeout the IO, force exit the process, callback signals and pump
/// synchronous IO engine
pub(crate) fn __asyncify<T, Fut>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    timeout: Option<Duration>,
    work: Fut,
) -> Result<T, Errno>
where
    T: 'static,
    Fut: std::future::Future<Output = Result<T, Errno>>,
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
                let _ = tx_ret.send(ret);
            },
            // If a signaller is triggered then we interrupt the main process
            _ = signaler.recv() => {
                if let Err(err) = ctx.data().clone().process_signals(ctx) {
                    let _ = tx_ret.send(Err(err));
                }
            },
            // Optional timeout
            _ = timeout => {
                let _ = tx_ret.send(Err(Errno::Timedout));
            },
        }
    }));

    // If a signal is received then we need to process it and if
    // we can not then fail with an interrupt error code
    rx_ret.try_recv().map_err(|_| Errno::Intr)?
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

/// Performs an immuatble operation on the socket while running in an asynchronous runtime
/// This has built in signal support
pub(crate) fn __sock_actor<T, F, Fut>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<T, Errno>
where
    T: 'static,
    F: FnOnce(crate::net::socket::InodeSocket) -> Fut + 'static,
    Fut: std::future::Future<Output = Result<T, Errno>>,
{
    let env = ctx.data();
    let tasks = env.tasks.clone();

    let state = env.state.clone();
    let inodes = state.inodes.clone();

    let fd_entry = state.fs.get_fd(sock)?;
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

            // Start the work using the socket
            let work = actor(socket);

            // Block on the work and process it
            let (tx, rx) = std::sync::mpsc::channel();
            tasks.block_on(Box::pin(async move {
                let ret = work.await;
                tx.send(ret);
            }));
            rx.recv().unwrap()
        }
        _ => {
            return Err(Errno::Notsock);
        }
    }
}

/// Performs mutable work on a socket under an asynchronous runtime with
/// built in signal processing
pub(crate) fn __sock_actor_mut<'a, T, F, Fut>(
    ctx: &'a mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<T, Errno>
where
    T: 'static,
    F: FnOnce(crate::net::socket::InodeSocket) -> Fut,
    Fut: std::future::Future<Output = Result<T, Errno>> + 'a,
{
    let env = ctx.data();
    let tasks = env.tasks.clone();

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

                // Start the work using the socket
                let work = actor(socket);

                // Block on the work and process it
                let (tx, rx) = std::sync::mpsc::channel();
                tasks.block_on(Box::pin(async move {
                    let ret = work.await;
                    tx.send(ret);
                }));
                rx.recv().unwrap()
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
pub(crate) fn __sock_upgrade<'a, F, Fut>(
    ctx: &'a mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    rights: Rights,
    actor: F,
) -> Result<(), Errno>
where
    F: FnOnce(crate::net::socket::InodeSocket) -> Fut,
    Fut: std::future::Future<Output = Result<Option<crate::net::socket::InodeSocket>, Errno>> + 'a,
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

                // Start the work using the socket
                let work = actor(socket);

                // Block on the work and process it
                let (tx, rx) = std::sync::mpsc::channel();
                tasks.block_on(Box::pin(async move {
                    let ret = work.await;
                    tx.send(ret);
                }));
                let new_socket = rx.recv().unwrap()?;

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
pub(crate) fn write_buffer_array<M: MemorySize>(
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

pub(crate) fn get_current_time_in_nanos() -> Result<Timestamp, Errno> {
    let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000).unwrap() as u128;
    Ok(now as Timestamp)
}

pub(crate) fn get_stack_base(mut ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> u64 {
    ctx.data().stack_base
}

pub(crate) fn get_stack_start(mut ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> u64 {
    ctx.data().stack_start
}

pub(crate) fn get_memory_stack_pointer(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
) -> Result<u64, String> {
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

pub(crate) fn get_memory_stack_offset(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
) -> Result<u64, String> {
    let stack_base = get_stack_base(ctx);
    let stack_pointer = get_memory_stack_pointer(ctx)?;
    Ok(stack_base - stack_pointer)
}

pub(crate) fn set_memory_stack_offset(
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

#[allow(dead_code)]
pub(crate) fn get_memory_stack<M: MemorySize>(
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
pub(crate) fn set_memory_stack<M: MemorySize>(
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

#[must_use = "you must return the result immediately so the stack can unwind"]
pub(crate) fn unwind<M: MemorySize, F>(
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
pub(crate) fn rewind<M: MemorySize>(
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

pub(crate) fn handle_rewind<M: MemorySize>(ctx: &mut FunctionEnvMut<'_, WasiEnv>) -> bool {
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

// Function to prepare the WASI environment
pub(crate) fn _prepare_wasi(wasi_env: &mut WasiEnv, args: Option<Vec<String>>) {
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

pub(crate) fn conv_bus_err_to_exit_code(err: VirtualBusError) -> u32 {
    match err {
        VirtualBusError::AccessDenied => -1i32 as u32,
        VirtualBusError::NotFound => -2i32 as u32,
        VirtualBusError::Unsupported => -22i32 as u32,
        VirtualBusError::BadRequest | _ => -8i32 as u32,
    }
}

// Function for converting the format
pub(crate) fn conv_bus_format(format: BusDataFormat) -> BusDataFormat {
    match format {
        BusDataFormat::Raw => BusDataFormat::Raw,
        BusDataFormat::Bincode => BusDataFormat::Bincode,
        BusDataFormat::MessagePack => BusDataFormat::MessagePack,
        BusDataFormat::Json => BusDataFormat::Json,
        BusDataFormat::Yaml => BusDataFormat::Yaml,
        BusDataFormat::Xml => BusDataFormat::Xml,
        BusDataFormat::Rkyv => BusDataFormat::Rkyv,
    }
}

pub(crate) fn conv_bus_format_from(format: BusDataFormat) -> BusDataFormat {
    match format {
        BusDataFormat::Raw => BusDataFormat::Raw,
        BusDataFormat::Bincode => BusDataFormat::Bincode,
        BusDataFormat::MessagePack => BusDataFormat::MessagePack,
        BusDataFormat::Json => BusDataFormat::Json,
        BusDataFormat::Yaml => BusDataFormat::Yaml,
        BusDataFormat::Xml => BusDataFormat::Xml,
        BusDataFormat::Rkyv => BusDataFormat::Rkyv,
    }
}
