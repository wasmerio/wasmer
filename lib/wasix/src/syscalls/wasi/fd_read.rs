use std::{collections::VecDeque, task::Waker};

use virtual_fs::{AsyncReadExt, DeviceFile, ReadBuf};

use super::*;
use crate::{
    fs::NotificationInner,
    journal::SnapshotTrigger,
    net::socket::TimeType,
    os::task::process::{MaybeCheckpointResult, WasiProcessCheckpoint, WasiProcessInner},
    syscalls::*,
};

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
#[instrument(level = "trace", skip_all, fields(%fd, nread = field::Empty), ret)]
pub fn fd_read<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    let fd_entry = {
        let env = ctx.data();
        let state = env.state.clone();
        wasi_try_ok!(state.fs.get_fd(fd))
    };
    let offset = fd_entry.inner.offset.load(Ordering::Acquire) as usize;

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);
    if fd == DeviceFile::STDIN {
        ctx = wasi_try_ok!(maybe_snapshot_once::<M>(ctx, SnapshotTrigger::FirstStdin)?);
    }

    let res = fd_read_internal::<M>(&mut ctx, fd, fd_entry, iovs, iovs_len, offset, nread, true)?;
    fd_read_internal_handler(ctx, res, nread)
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
#[instrument(level = "trace", skip_all, fields(%fd, %offset, ?nread), ret)]
pub fn fd_pread<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);
    if fd == DeviceFile::STDIN {
        ctx = wasi_try_ok!(maybe_snapshot_once::<M>(ctx, SnapshotTrigger::FirstStdin)?);
    }

    let fd_entry = {
        let env = ctx.data();
        let state = env.state.clone();
        wasi_try_ok!(state.fs.get_fd(fd))
    };
    let res = fd_read_internal::<M>(
        &mut ctx,
        fd,
        fd_entry,
        iovs,
        iovs_len,
        offset as usize,
        nread,
        false,
    )?;
    fd_read_internal_handler::<M>(ctx, res, nread)
}

pub(crate) fn fd_read_internal_handler<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    res: Result<usize, Errno>,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let mut ret = Errno::Success;
    let bytes_read = match res {
        Ok(bytes_read) => bytes_read,
        Err(err) => {
            ret = err;
            0
        }
    };
    Span::current().record("nread", bytes_read);

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let nread_ref = nread.deref(&memory);
    wasi_try_mem_ok!(nread_ref.write(bytes_read));

    Ok(ret)
}

/// Extracts validated iov buffer specs from WASM memory as raw host pointers,
/// releasing the `MemoryView` borrow before returning so that `ctx` can be
/// passed exclusively to `__asyncify`.
///
/// # Safety
/// The returned `*mut u8` base pointer points into WASM linear memory.
/// It remains valid as long as the memory is not grown. Callers must only use
/// the returned pointers while the calling thread is blocked inside
/// `__asyncify` / `block_on`, where WASM execution (and thus `memory.grow`)
/// cannot occur on this thread.
unsafe fn extract_iov_bufs<M: MemorySize>(
    ctx: &FunctionEnvMut<'_, WasiEnv>,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
) -> Result<(*mut u8, Vec<(usize, usize)>), Errno> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(ctx) };
    let base = memory.data_ptr();
    let mem_size = memory.data_size() as usize;

    let iovs_arr = iovs.slice(&memory, iovs_len).map_err(mem_error_to_wasi)?;
    let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;

    let specs = iovs_arr
        .iter()
        .map(|iov| {
            let buf_offset: usize = iov.buf.try_into().map_err(|_| Errno::Overflow)?;
            let buf_len: usize = iov.buf_len.try_into().map_err(|_| Errno::Overflow)?;
            if buf_offset.saturating_add(buf_len) > mem_size {
                return Err(Errno::Fault);
            }
            Ok((buf_offset, buf_len))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // `iovs_arr` and `memory` drop here, releasing the borrow on `ctx`.
    Ok((base, specs))
}

#[allow(clippy::await_holding_lock)]
pub(crate) fn fd_read_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    fd_entry: Fd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: usize,
    nread: WasmPtr<M::Offset, M>,
    should_update_cursor: bool,
) -> WasiResult<usize> {
    let is_stdio = fd_entry.is_stdio;

    if !is_stdio && !fd_entry.inner.rights.contains(Rights::FD_READ) {
        // TODO: figure out the error to return when lacking rights
        return Ok(Err(Errno::Access));
    }

    let inode = fd_entry.inode;
    let fd_flags = fd_entry.inner.flags;
    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);
    let asyncify_timeout = if nonblocking {
        Some(Duration::ZERO)
    } else {
        None
    };

    // Extract iov buffer specs (as raw pointers + lengths) from WASM memory
    // before releasing the memory borrow. This allows `ctx` to be passed
    // exclusively into `__asyncify`, which provides signal and DL-op handling.
    //
    // Safety: we only use these pointers inside `__asyncify`, which drives the
    // future synchronously via `block_on`, parking this thread. While parked,
    // WASM cannot execute `memory.grow`, so the base pointer is stable.
    let (mem_base, iov_specs) =
        wasi_try_ok_ok!(unsafe { extract_iov_bufs::<M>(ctx, iovs, iovs_len) });

    let (bytes_read, can_update_cursor) = {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                let Some(handle) = handle else {
                    tracing::warn!("fd_read: file handle is None");
                    return Ok(Err(Errno::Badf));
                };
                let handle = handle.clone();
                drop(guard);

                let res = __asyncify(ctx, asyncify_timeout, async move {
                    let mut handle = handle.write().map_err(|_| Errno::Fault)?;
                    if !is_stdio {
                        handle
                            .seek(std::io::SeekFrom::Start(offset as u64))
                            .await
                            .map_err(map_io_err)?;
                    }

                    let mut total_read = 0usize;
                    for (buf_offset, buf_len) in iov_specs {
                        let buf = unsafe {
                            std::slice::from_raw_parts_mut(mem_base.add(buf_offset), buf_len)
                        };
                        let local_read =
                            handle.read(buf).await.map_err(
                                |err| match From::<std::io::Error>::from(err) {
                                    Errno::Again if is_stdio => Errno::Badf,
                                    e => e,
                                },
                            );
                        let local_read = match local_read {
                            Ok(n) => n,
                            Err(_) if total_read > 0 => break,
                            Err(err) => return Err(err),
                        };
                        total_read += local_read;
                        if local_read < buf_len {
                            break;
                        }
                    }
                    Ok(total_read)
                });
                let read = wasi_try_ok_ok!(res?.map_err(|err| match err {
                    Errno::Timedout => Errno::Again,
                    a => a,
                }));
                (read, true)
            }
            Kind::Socket { socket } => {
                let socket = socket.clone();
                drop(guard);

                let timeout = socket
                    .opt_time(TimeType::ReadTimeout)
                    .ok()
                    .flatten()
                    .unwrap_or(Duration::from_secs(30));
                let tasks = ctx.data().tasks().clone();

                let res = __asyncify(ctx, asyncify_timeout, async move {
                    let mut total_read = 0usize;
                    for (buf_offset, buf_len) in iov_specs {
                        let buf = unsafe {
                            std::slice::from_raw_parts_mut(
                                mem_base.add(buf_offset) as *mut std::mem::MaybeUninit<u8>,
                                buf_len,
                            )
                        };
                        let local_read = socket
                            .recv(tasks.deref(), buf, Some(timeout), nonblocking, false)
                            .await?;
                        total_read += local_read;
                        // A zero-byte return signals connection closed (EOF);
                        // a short read is normal for stream sockets and does NOT
                        // indicate end-of-stream.
                        if local_read == 0 {
                            break;
                        }
                    }
                    Ok(total_read)
                });
                let res = res?.map_err(|err| match err {
                    Errno::Timedout => Errno::Again,
                    a => a,
                });
                match res {
                    Err(Errno::Connaborted) | Err(Errno::Connreset) => (0, false),
                    res => {
                        let bytes_read = wasi_try_ok_ok!(res);
                        (bytes_read, false)
                    }
                }
            }
            Kind::PipeTx { .. } => return Ok(Err(Errno::Badf)),
            Kind::PipeRx { rx } => {
                let mut rx = rx.clone();
                drop(guard);

                let res = __asyncify(ctx, asyncify_timeout, async move {
                    let mut total_read = 0usize;
                    for (buf_offset, buf_len) in iov_specs {
                        let buf = unsafe {
                            std::slice::from_raw_parts_mut(mem_base.add(buf_offset), buf_len)
                        };
                        let local_read = if nonblocking {
                            rx.try_read(buf).ok_or(Errno::Again)?
                        } else {
                            virtual_fs::AsyncReadExt::read(&mut rx, buf).await?
                        };
                        total_read += local_read;
                        if local_read < buf_len {
                            break;
                        }
                    }
                    Ok(total_read)
                });
                let bytes_read = wasi_try_ok_ok!(res?.map_err(|err| match err {
                    Errno::Timedout => Errno::Again,
                    a => a,
                }));
                (bytes_read, false)
            }
            Kind::DuplexPipe { pipe } => {
                let mut pipe = pipe.clone();
                drop(guard);

                let res = __asyncify(ctx, asyncify_timeout, async move {
                    let mut total_read = 0usize;
                    for (buf_offset, buf_len) in iov_specs {
                        let buf = unsafe {
                            std::slice::from_raw_parts_mut(mem_base.add(buf_offset), buf_len)
                        };
                        let local_read = if nonblocking {
                            pipe.try_read(buf).ok_or(Errno::Again)?
                        } else {
                            virtual_fs::AsyncReadExt::read(&mut pipe, buf).await?
                        };
                        total_read += local_read;
                        if local_read < buf_len {
                            break;
                        }
                    }
                    Ok(total_read)
                });
                let bytes_read = wasi_try_ok_ok!(res?.map_err(|err| match err {
                    Errno::Timedout => Errno::Again,
                    a => a,
                }));
                (bytes_read, false)
            }
            Kind::Dir { .. } | Kind::Root { .. } => {
                // TODO: verify
                return Ok(Err(Errno::Isdir));
            }
            Kind::EventNotifications { inner } => {
                struct NotifyPoller {
                    inner: Arc<NotificationInner>,
                    non_blocking: bool,
                }
                impl Future for NotifyPoller {
                    type Output = Result<u64, Errno>;
                    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                        if self.non_blocking {
                            Poll::Ready(self.inner.try_read().ok_or(Errno::Again))
                        } else {
                            self.inner.read(cx.waker()).map(Ok)
                        }
                    }
                }

                let poller = NotifyPoller {
                    inner: inner.clone(),
                    non_blocking: nonblocking,
                };
                drop(guard);

                let res = __asyncify(ctx, None, poller)?.map_err(|err| match err {
                    Errno::Timedout => Errno::Again,
                    a => a,
                });
                let val = wasi_try_ok_ok!(res);

                let env = ctx.data();
                let memory = unsafe { env.memory_view(ctx) };
                let reader = val.to_ne_bytes();
                let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                let ret = wasi_try_ok_ok!(read_bytes(&reader[..], &memory, iovs_arr));
                (ret, false)
            }
            Kind::Symlink { .. } | Kind::Epoll { .. } => {
                return Ok(Err(Errno::Notsup));
            }
            Kind::Buffer { buffer } => {
                let env = ctx.data();
                let memory = unsafe { env.memory_view(ctx) };
                let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                let read = wasi_try_ok_ok!(read_bytes(&buffer[offset..], &memory, iovs_arr));
                (read, true)
            }
        }
    };

    if !is_stdio && should_update_cursor && can_update_cursor {
        fd_entry
            .inner
            .offset
            .fetch_add(bytes_read as u64, Ordering::AcqRel);
    }

    Ok(Ok(bytes_read))
}
