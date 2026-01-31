use std::{collections::VecDeque, task::Waker};

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

    let offset = {
        let mut env = ctx.data();
        let state = env.state.clone();
        let inodes = state.inodes.clone();

        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
        fd_entry.inner.offset.load(Ordering::Acquire) as usize
    };

    ctx = wasi_try_ok!(maybe_backoff::<M>(ctx)?);
    if fd == __WASI_STDIN_FILENO {
        ctx = wasi_try_ok!(maybe_snapshot_once::<M>(ctx, SnapshotTrigger::FirstStdin)?);
    }

    let res = fd_read_internal::<M>(&mut ctx, fd, iovs, iovs_len, offset, nread, true)?;
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
    if fd == __WASI_STDIN_FILENO {
        ctx = wasi_try_ok!(maybe_snapshot_once::<M>(ctx, SnapshotTrigger::FirstStdin)?);
    }

    let res = fd_read_internal::<M>(&mut ctx, fd, iovs, iovs_len, offset as usize, nread, false)?;
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

#[cfg(feature = "legacy-fs")]
#[allow(clippy::await_holding_lock)]
pub(crate) fn fd_read_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: usize,
    nread: WasmPtr<M::Offset, M>,
    should_update_cursor: bool,
) -> WasiResult<usize> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let state = env.state();

    let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(fd));
    let is_stdio = fd_entry.is_stdio;

    let bytes_read = {
        if !is_stdio && !fd_entry.inner.rights.contains(Rights::FD_READ) {
            // TODO: figure out the error to return when lacking rights
            return Ok(Err(Errno::Access));
        }

        let inode = fd_entry.inode;
        let fd_flags = fd_entry.inner.flags;

        let (bytes_read, can_update_cursor) = {
            let mut guard = inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let handle = handle.clone();

                        drop(guard);

                        let res = __asyncify_light(
                            env,
                            if fd_flags.contains(Fdflags::NONBLOCK) {
                                Some(Duration::ZERO)
                            } else {
                                None
                            },
                            async move {
                                let mut handle = match handle.write() {
                                    Ok(a) => a,
                                    Err(_) => return Err(Errno::Fault),
                                };
                                if !is_stdio {
                                    handle
                                        .seek(std::io::SeekFrom::Start(offset as u64))
                                        .await
                                        .map_err(map_io_err)?;
                                }

                                let mut total_read = 0usize;

                                let iovs_arr =
                                    iovs.slice(&memory, iovs_len).map_err(mem_error_to_wasi)?;
                                let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
                                for iovs in iovs_arr.iter() {
                                    let mut buf = WasmPtr::<u8, M>::new(iovs.buf)
                                        .slice(&memory, iovs.buf_len)
                                        .map_err(mem_error_to_wasi)?
                                        .access()
                                        .map_err(mem_error_to_wasi)?;
                                    let r = handle.read(buf.as_mut()).await.map_err(|err| {
                                        let err = From::<std::io::Error>::from(err);
                                        match err {
                                            Errno::Again => {
                                                if is_stdio {
                                                    Errno::Badf
                                                } else {
                                                    Errno::Again
                                                }
                                            }
                                            a => a,
                                        }
                                    });
                                    let local_read = match r {
                                        Ok(s) => s,
                                        Err(_) if total_read > 0 => break,
                                        Err(err) => return Err(err),
                                    };
                                    total_read += local_read;
                                    if local_read != buf.len() {
                                        break;
                                    }
                                }
                                Ok(total_read)
                            },
                        );
                        let read = wasi_try_ok_ok!(res?.map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }));
                        (read, true)
                    } else {
                        return Ok(Err(Errno::Badf));
                    }
                }
                Kind::Socket { socket } => {
                    let socket = socket.clone();

                    drop(guard);

                    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);
                    let timeout = socket
                        .opt_time(TimeType::ReadTimeout)
                        .ok()
                        .flatten()
                        .unwrap_or(Duration::from_secs(30));

                    let tasks = env.tasks().clone();
                    let res = __asyncify_light(
                        env,
                        if fd_flags.contains(Fdflags::NONBLOCK) {
                            Some(Duration::ZERO)
                        } else {
                            None
                        },
                        async move {
                            let mut total_read = 0usize;

                            let iovs_arr =
                                iovs.slice(&memory, iovs_len).map_err(mem_error_to_wasi)?;
                            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
                            for iovs in iovs_arr.iter() {
                                let mut buf = WasmPtr::<u8, M>::new(iovs.buf)
                                    .slice(&memory, iovs.buf_len)
                                    .map_err(mem_error_to_wasi)?
                                    .access()
                                    .map_err(mem_error_to_wasi)?;

                                let local_read = socket
                                    .recv(
                                        tasks.deref(),
                                        buf.as_mut_uninit(),
                                        Some(timeout),
                                        nonblocking,
                                        false,
                                    )
                                    .await?;
                                total_read += local_read;
                                if total_read != buf.len() {
                                    break;
                                }
                            }
                            Ok(total_read)
                        },
                    );
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

                    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);

                    let res = __asyncify_light(
                        env,
                        if fd_flags.contains(Fdflags::NONBLOCK) {
                            Some(Duration::ZERO)
                        } else {
                            None
                        },
                        async move {
                            let mut total_read = 0usize;

                            let iovs_arr =
                                iovs.slice(&memory, iovs_len).map_err(mem_error_to_wasi)?;
                            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
                            for iovs in iovs_arr.iter() {
                                let mut buf = WasmPtr::<u8, M>::new(iovs.buf)
                                    .slice(&memory, iovs.buf_len)
                                    .map_err(mem_error_to_wasi)?
                                    .access()
                                    .map_err(mem_error_to_wasi)?;

                                let local_read = match nonblocking {
                                    true => match rx.try_read(buf.as_mut()) {
                                        Some(amt) => amt,
                                        None => {
                                            return Err(Errno::Again);
                                        }
                                    },
                                    false => {
                                        virtual_fs::AsyncReadExt::read(&mut rx, buf.as_mut())
                                            .await?
                                    }
                                };
                                total_read += local_read;
                                if local_read != buf.len() {
                                    break;
                                }
                            }
                            Ok(total_read)
                        },
                    );

                    let bytes_read = wasi_try_ok_ok!(res?.map_err(|err| match err {
                        Errno::Timedout => Errno::Again,
                        a => a,
                    }));

                    (bytes_read, false)
                }
                Kind::DuplexPipe { pipe } => {
                    let mut pipe = pipe.clone();
                    drop(guard);

                    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);

                    let res = __asyncify_light(
                        env,
                        if fd_flags.contains(Fdflags::NONBLOCK) {
                            Some(Duration::ZERO)
                        } else {
                            None
                        },
                        async move {
                            let mut total_read = 0usize;

                            let iovs_arr =
                                iovs.slice(&memory, iovs_len).map_err(mem_error_to_wasi)?;
                            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
                            for iovs in iovs_arr.iter() {
                                let mut buf = WasmPtr::<u8, M>::new(iovs.buf)
                                    .slice(&memory, iovs.buf_len)
                                    .map_err(mem_error_to_wasi)?
                                    .access()
                                    .map_err(mem_error_to_wasi)?;

                                let local_read = match nonblocking {
                                    true => match pipe.try_read(buf.as_mut()) {
                                        Some(amt) => amt,
                                        None => {
                                            return Err(Errno::Again);
                                        }
                                    },
                                    false => {
                                        virtual_fs::AsyncReadExt::read(&mut pipe, buf.as_mut())
                                            .await?
                                    }
                                };
                                total_read += local_read;
                                if local_read != buf.len() {
                                    break;
                                }
                            }
                            Ok(total_read)
                        },
                    );

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
                    // Create a poller
                    struct NotifyPoller {
                        inner: Arc<NotificationInner>,
                        non_blocking: bool,
                    }
                    let poller = NotifyPoller {
                        inner: inner.clone(),
                        non_blocking: fd_flags.contains(Fdflags::NONBLOCK),
                    };

                    drop(guard);

                    // The poller will register itself for notifications and wait for the
                    // counter to drop
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

                    // Yield until the notifications are triggered
                    let tasks_inner = env.tasks().clone();

                    let res = __asyncify_light(env, None, poller)?.map_err(|err| match err {
                        Errno::Timedout => Errno::Again,
                        a => a,
                    });
                    let val = wasi_try_ok_ok!(res);

                    let mut memory = unsafe { env.memory_view(ctx) };
                    let reader = val.to_ne_bytes();
                    let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                    let ret = wasi_try_ok_ok!(read_bytes(&reader[..], &memory, iovs_arr));
                    (ret, false)
                }
                Kind::Symlink { .. } | Kind::Epoll { .. } => {
                    return Ok(Err(Errno::Notsup));
                }
                Kind::Buffer { buffer } => {
                    let memory = unsafe { env.memory_view(ctx) };
                    let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                    let read = wasi_try_ok_ok!(read_bytes(&buffer[offset..], &memory, iovs_arr));
                    (read, true)
                }
            }
        };

        if !is_stdio && should_update_cursor && can_update_cursor {
            // reborrow
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok_ok!(fd_map.get_mut(fd).ok_or(Errno::Badf));
            let old = fd_entry
                .offset
                .fetch_add(bytes_read as u64, Ordering::AcqRel);
        }

        bytes_read
    };

    Ok(Ok(bytes_read))
}

#[cfg(not(feature = "legacy-fs"))]
#[allow(clippy::await_holding_lock)]
pub(crate) fn fd_read_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: usize,
    nread: WasmPtr<M::Offset, M>,
    should_update_cursor: bool,
) -> WasiResult<usize> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let state = env.state();

    let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(fd));
    if !fd_entry.is_stdio && !fd_entry.inner.rights.contains(Rights::FD_READ) {
        return Ok(Err(Errno::Access));
    }

    let iovs_arr = iovs.slice(&memory, iovs_len).map_err(mem_error_to_wasi)?;
    let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
    let fd_flags = fd_entry.inner.flags;

    let res = match fd_entry.kind {
        Kind::VfsFile { handle } => {
            let handle = handle.clone();
            __asyncify_light(
                env,
                if fd_flags.contains(Fdflags::NONBLOCK) {
                    Some(Duration::ZERO)
                } else {
                    None
                },
                async move {
                    let mut total_read = 0usize;
                    for iov in iovs_arr.iter() {
                        let mut buf = WasmPtr::<u8, M>::new(iov.buf)
                            .slice(&memory, iov.buf_len)
                            .map_err(mem_error_to_wasi)?
                            .access()
                            .map_err(mem_error_to_wasi)?;
                        let read = if should_update_cursor {
                            handle.read(buf.as_mut()).await
                        } else {
                            handle
                                .pread_at((offset + total_read) as u64, buf.as_mut())
                                .await
                        }
                        .map_err(|err| vfs_unix::errno::vfs_error_to_wasi_errno(&err))?;
                        total_read += read;
                        if read != buf.len() {
                            break;
                        }
                    }
                    Ok(total_read)
                },
            )
        }
        Kind::Stdin { handle } => {
            let handle = handle.clone();
            __asyncify_light(env, None, async move {
                let mut total_read = 0usize;
                for iov in iovs_arr.iter() {
                    let mut buf = WasmPtr::<u8, M>::new(iov.buf)
                        .slice(&memory, iov.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    let read = handle.read(buf.as_mut()).await?;
                    total_read += read;
                    if read != buf.len() {
                        break;
                    }
                }
                Ok(total_read)
            })
        }
        Kind::PipeRx { rx } => {
            let rx = rx.clone();
            __asyncify_light(env, None, async move {
                let mut total_read = 0usize;
                for iov in iovs_arr.iter() {
                    let mut buf = WasmPtr::<u8, M>::new(iov.buf)
                        .slice(&memory, iov.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    let read = rx.read(buf.as_mut()).await?;
                    total_read += read;
                    if read != buf.len() {
                        break;
                    }
                }
                Ok(total_read)
            })
        }
        Kind::DuplexPipe { pipe } => {
            let pipe = pipe.clone();
            __asyncify_light(env, None, async move {
                let mut total_read = 0usize;
                for iov in iovs_arr.iter() {
                    let mut buf = WasmPtr::<u8, M>::new(iov.buf)
                        .slice(&memory, iov.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    let read = pipe.read(buf.as_mut()).await?;
                    total_read += read;
                    if read != buf.len() {
                        break;
                    }
                }
                Ok(total_read)
            })
        }
        Kind::EventNotifications { inner } => {
            let inner = inner.clone();
            __asyncify_light(env, None, async move {
                let mut total_read = 0usize;
                for iov in iovs_arr.iter() {
                    let mut buf = WasmPtr::<u8, M>::new(iov.buf)
                        .slice(&memory, iov.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    let val = match inner.read() {
                        Some(val) => val,
                        None => break,
                    };
                    let data = val.to_ne_bytes();
                    let to_copy = data.len().min(buf.len());
                    buf.as_mut()[..to_copy].copy_from_slice(&data[..to_copy]);
                    total_read += to_copy;
                    break;
                }
                Ok(total_read)
            })
        }
        Kind::Buffer { buffer } => {
            let mut total_read = 0usize;
            for iov in iovs_arr.iter() {
                let mut buf = WasmPtr::<u8, M>::new(iov.buf)
                    .slice(&memory, iov.buf_len)
                    .map_err(mem_error_to_wasi)?
                    .access()
                    .map_err(mem_error_to_wasi)?;
                let remaining = &buffer[total_read..];
                let to_copy = remaining.len().min(buf.len());
                buf.as_mut()[..to_copy].copy_from_slice(&remaining[..to_copy]);
                total_read += to_copy;
                if to_copy != buf.len() {
                    break;
                }
            }
            Ok(total_read)
        }
        _ => Ok(Err(Errno::Badf)),
    };

    let bytes_read = wasi_try_ok_ok!(res?.map_err(|err| match err {
        Errno::Timedout => Errno::Again,
        other => other,
    }));

    let nread_ref = nread.deref(&memory);
    wasi_try_mem_ok!(nread_ref.write(bytes_read as M::Offset));
    Ok(Ok(bytes_read))
}
