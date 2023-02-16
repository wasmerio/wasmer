use super::*;
use crate::syscalls::*;

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
        fd,
    );

    let offset = {
        let mut env = ctx.data();
        let state = env.state.clone();
        let inodes = state.inodes.clone();

        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
        fd_entry.offset.load(Ordering::Acquire) as usize
    };

    fd_write_internal::<M>(ctx, fd, iovs, iovs_len, offset, nwritten, true)
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

    fd_write_internal::<M>(ctx, fd, iovs, iovs_len, offset as usize, nwritten, false)
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
fn fd_write_internal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: usize,
    nwritten: WasmPtr<M::Offset, M>,
    should_update_cursor: bool,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let mut env = ctx.data();
    let state = env.state.clone();
    let mut memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(iovs.slice(&memory, iovs_len));

    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
    let is_stdio = fd_entry.is_stdio;

    let bytes_written = {
        if !is_stdio && !fd_entry.rights.contains(Rights::FD_WRITE) {
            return Ok(Errno::Access);
        }

        let fd_flags = fd_entry.flags;

        let (bytes_written, can_update_cursor) = {
            let (mut memory, _) = env.get_memory_and_wasi_state(&ctx, 0);
            let mut guard = fd_entry.inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let handle = handle.clone();
                        drop(guard);

                        let buf_len: M::Offset = iovs_arr
                            .iter()
                            .filter_map(|a| a.read().ok())
                            .map(|a| a.buf_len)
                            .sum();
                        let buf_len: usize =
                            wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
                        let mut buf = Vec::with_capacity(buf_len);
                        wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));

                        let written = wasi_try_ok!(__asyncify(
                            &mut ctx,
                            if fd_entry.flags.contains(Fdflags::NONBLOCK) {
                                Some(Duration::ZERO)
                            } else {
                                None
                            },
                            async {
                                let mut handle = handle.write().unwrap();
                                if !is_stdio {
                                    handle
                                        .seek(std::io::SeekFrom::Start(offset as u64))
                                        .await
                                        .map_err(map_io_err)?;
                                }

                                handle.write(&buf[..]).await.map_err(map_io_err)
                            }
                        )?
                        .map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }));

                        (written, true)
                    } else {
                        return Ok(Errno::Inval);
                    }
                }
                Kind::Socket { socket } => {
                    let socket = socket.clone();
                    drop(guard);

                    let buf_len: M::Offset = iovs_arr
                        .iter()
                        .filter_map(|a| a.read().ok())
                        .map(|a| a.buf_len)
                        .sum();
                    let buf_len: usize = wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
                    let mut buf = Vec::with_capacity(buf_len);
                    wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));

                    let tasks = env.tasks().clone();
                    let written = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
                        socket.send(tasks.deref(), &buf, fd_flags).await
                    })?);
                    (written, false)
                }
                Kind::Pipe { pipe } => {
                    let buf_len: M::Offset = iovs_arr
                        .iter()
                        .filter_map(|a| a.read().ok())
                        .map(|a| a.buf_len)
                        .sum();
                    let buf_len: usize = wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
                    let mut buf = Vec::with_capacity(buf_len);
                    wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));

                    let written =
                        wasi_try_ok!(std::io::Write::write(pipe, &buf[..]).map_err(map_io_err));
                    (written, false)
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(Errno::Isdir);
                }
                Kind::EventNotifications(inner) => {
                    let mut val: [MaybeUninit<u8>; 8] =
                        unsafe { MaybeUninit::uninit().assume_init() };
                    let written = wasi_try_ok!(copy_to_slice(&memory, iovs_arr, &mut val[..]));
                    if written != val.len() {
                        return Ok(Errno::Inval);
                    }
                    let val = u64::from_ne_bytes(unsafe { std::mem::transmute(val) });

                    inner.write(val);

                    (written, false)
                }
                Kind::Symlink { .. } => return Ok(Errno::Inval),
                Kind::Buffer { buffer } => {
                    let written =
                        wasi_try_ok!(write_bytes(&mut buffer[offset..], &memory, iovs_arr));
                    (written, true)
                }
            }
        };
        env = ctx.data();
        memory = env.memory_view(&ctx);

        // reborrow and update the size
        if !is_stdio {
            if can_update_cursor && should_update_cursor {
                let mut fd_map = state.fs.fd_map.write().unwrap();
                let fd_entry = wasi_try_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
                fd_entry
                    .offset
                    .fetch_add(bytes_written as u64, Ordering::AcqRel);
            }

            // we set the size but we don't return any errors if it fails as
            // pipes and sockets will not do anything with this
            let (mut memory, _, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
            // Cast is valid because we don't support 128 bit systems...
            fd_entry.inode.stat.write().unwrap().st_size += bytes_written as u64;
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
