use std::{collections::VecDeque, task::Waker};

use wasmer_vfs::{AsyncReadExt, ReadBuf};

use super::*;
use crate::{fs::NotificationInner, syscalls::*};

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
    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    let offset = {
        let mut env = ctx.data();
        let state = env.state.clone();
        let inodes = state.inodes.clone();

        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
        fd_entry.offset.load(Ordering::Acquire) as usize
    };

    let res = fd_read_internal::<M>(&mut ctx, fd, iovs, iovs_len, offset, nread, true)?;

    let mut ret = Errno::Success;
    let bytes_read = match res {
        Ok(bytes_read) => {
            trace!(
                %fd,
                %bytes_read,
                "wasi[{}:{}]::fd_read",
                ctx.data().pid(),
                ctx.data().tid(),
            );
            bytes_read
        }
        Err(err) => {
            let read_err = err.name();
            trace!(
                %fd,
                %read_err,
                "wasi[{}:{}]::fd_read",
                ctx.data().pid(),
                ctx.data().tid(),
            );
            ret = err;
            0
        }
    };

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let nread_ref = nread.deref(&memory);
    wasi_try_mem_ok!(nread_ref.write(bytes_read));

    Ok(ret)
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
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nread: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    let res = fd_read_internal::<M>(&mut ctx, fd, iovs, iovs_len, offset as usize, nread, false)?;

    let mut ret = Errno::Success;
    let bytes_read = match res {
        Ok(bytes_read) => {
            trace!(
                %fd,
                %offset,
                %bytes_read,
                "wasi[{}:{}]::fd_pread - {:?}",
                ctx.data().pid(),
                ctx.data().tid(),
                ret
            );
            bytes_read
        }
        Err(err) => {
            let read_err = err.name();
            trace!(
                %fd,
                %offset,
                %read_err,
                "wasi[{}:{}]::fd_pread - {:?}",
                ctx.data().pid(),
                ctx.data().tid(),
                ret
            );
            ret = err;
            0
        }
    };

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let nread_ref = nread.deref(&memory);
    wasi_try_mem_ok!(nread_ref.write(bytes_read));

    Ok(ret)
}

fn fd_read_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_iovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: usize,
    nread: WasmPtr<M::Offset, M>,
    should_update_cursor: bool,
) -> Result<Result<usize, Errno>, WasiError> {
    wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);

    let mut env = ctx.data();
    let state = env.state.clone();

    let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(fd));
    let is_stdio = fd_entry.is_stdio;

    let bytes_read = {
        if !is_stdio && !fd_entry.rights.contains(Rights::FD_READ) {
            // TODO: figure out the error to return when lacking rights
            return Ok(Err(Errno::Access));
        }

        let inode = fd_entry.inode;
        let fd_flags = fd_entry.flags;

        let max_size = {
            let memory = env.memory_view(ctx);
            let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
            let mut max_size = 0usize;
            for iovs in iovs_arr.iter() {
                let iovs = wasi_try_mem_ok_ok!(iovs.read());
                let buf_len: usize =
                    wasi_try_ok_ok!(iovs.buf_len.try_into().map_err(|_| Errno::Overflow));
                max_size += buf_len;
            }
            max_size
        };

        let (bytes_read, can_update_cursor) = {
            let mut guard = inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let handle = handle.clone();
                        drop(guard);

                        let data = wasi_try_ok_ok!(__asyncify(
                            ctx,
                            if fd_flags.contains(Fdflags::NONBLOCK) {
                                Some(Duration::ZERO)
                            } else {
                                None
                            },
                            async move {
                                let mut handle = handle.write().unwrap();
                                if !is_stdio {
                                    handle
                                        .seek(std::io::SeekFrom::Start(offset as u64))
                                        .await
                                        .map_err(map_io_err)?;
                                }

                                let mut buf = Vec::with_capacity(max_size);

                                let amt = handle.read_buf(&mut buf).await.map_err(|err| {
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
                                })?;
                                Ok(buf)
                            }
                        )?
                        .map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }));
                        env = ctx.data();

                        let memory = env.memory_view(&ctx);
                        let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                        let read = wasi_try_ok_ok!(read_bytes(&data[..], &memory, iovs_arr));
                        (read, true)
                    } else {
                        return Ok(Err(Errno::Badf));
                    }
                }
                Kind::Socket { socket } => {
                    let socket = socket.clone();

                    drop(guard);

                    let tasks = env.tasks().clone();
                    let res = __asyncify(
                        ctx,
                        if fd_flags.contains(Fdflags::NONBLOCK) {
                            Some(Duration::ZERO)
                        } else {
                            None
                        },
                        async {
                            let mut buf = Vec::with_capacity(max_size);
                            unsafe {
                                buf.set_len(max_size);
                            }
                            socket
                                .recv(tasks.deref(), &mut buf, fd_flags)
                                .await
                                .map(|amt| {
                                    unsafe {
                                        buf.set_len(amt);
                                    }
                                    let buf: Vec<u8> = unsafe { std::mem::transmute(buf) };
                                    buf
                                })
                        },
                    )?
                    .map_err(|err| match err {
                        Errno::Timedout => Errno::Again,
                        a => a,
                    });
                    match res {
                        Err(Errno::Connaborted) | Err(Errno::Connreset) => (0, false),
                        res => {
                            let data = wasi_try_ok_ok!(res);
                            env = ctx.data();

                            let data_len = data.len();
                            let mut reader = &data[..];
                            let memory = env.memory_view(&ctx);
                            let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                            let bytes_read = wasi_try_ok_ok!(
                                read_bytes(reader, &memory, iovs_arr).map(|_| data_len)
                            );
                            (bytes_read, false)
                        }
                    }
                }
                Kind::Pipe { pipe } => {
                    let mut pipe = pipe.clone();

                    drop(guard);

                    let data = wasi_try_ok_ok!(__asyncify(
                        ctx,
                        if fd_flags.contains(Fdflags::NONBLOCK) {
                            Some(Duration::ZERO)
                        } else {
                            None
                        },
                        async move {
                            // TODO: optimize with MaybeUninit
                            let mut data = vec![0u8; max_size];
                            let amt = wasmer_vfs::AsyncReadExt::read(&mut pipe, &mut data[..])
                                .await
                                .map_err(map_io_err)?;
                            data.truncate(amt);
                            Ok(data)
                        }
                    )?
                    .map_err(|err| match err {
                        Errno::Timedout => Errno::Again,
                        a => a,
                    }));
                    env = ctx.data();

                    let data_len = data.len();
                    let mut reader = &data[..];

                    let memory = env.memory_view(ctx);
                    let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                    let bytes_read =
                        wasi_try_ok_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| data_len));
                    (bytes_read, false)
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(Err(Errno::Isdir));
                }
                Kind::EventNotifications(inner) => {
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
                    let val = wasi_try_ok_ok!(__asyncify(ctx, None, async { poller.await })?
                        .map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }));
                    env = ctx.data();

                    let mut memory = env.memory_view(ctx);
                    let reader = val.to_ne_bytes();
                    let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                    let ret = wasi_try_ok_ok!(read_bytes(&reader[..], &memory, iovs_arr));
                    (ret, false)
                }
                Kind::Symlink { .. } => unimplemented!("Symlinks in wasi::fd_read"),
                Kind::Buffer { buffer } => {
                    let memory = env.memory_view(ctx);
                    let iovs_arr = wasi_try_mem_ok_ok!(iovs.slice(&memory, iovs_len));
                    let read = wasi_try_ok_ok!(read_bytes(&buffer[offset..], &memory, iovs_arr));
                    (read, true)
                }
            }
        };

        if !is_stdio && should_update_cursor && can_update_cursor {
            // reborrow
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            let old = fd_entry
                .offset
                .fetch_add(bytes_read as u64, Ordering::AcqRel);
        }

        bytes_read
    };

    Ok(Ok(bytes_read))
}
