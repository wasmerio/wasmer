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
#[instrument(level = "trace", skip_all, fields(%fd, nwritten = field::Empty), ret, err)]
pub fn fd_write<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
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
#[instrument(level = "trace", skip_all, fields(%fd, %offset, nwritten = field::Empty), ret, err)]
pub fn fd_pwrite<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
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
            let iovs_arr = wasi_try_mem_ok!(iovs_arr.access());

            let (mut memory, _) = env.get_memory_and_wasi_state(&ctx, 0);
            let mut guard = fd_entry.inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let handle = handle.clone();
                        drop(guard);

                        let written = wasi_try_ok!(__asyncify_light(
                            env,
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

                                let mut written = 0usize;
                                for iovs in iovs_arr.iter() {
                                    let buf = WasmPtr::<u8, M>::new(iovs.buf)
                                        .slice(&memory, iovs.buf_len)
                                        .map_err(mem_error_to_wasi)?
                                        .access()
                                        .map_err(mem_error_to_wasi)?;
                                    let local_written = match handle.write(buf.as_ref()).await {
                                        Ok(s) => s,
                                        Err(_) if written > 0 => break,
                                        Err(err) => return Err(map_io_err(err)),
                                    };
                                    written += local_written;
                                    if local_written != buf.len() {
                                        break;
                                    }
                                }
                                if is_stdio {
                                    handle.flush().await.map_err(map_io_err)?;
                                }
                                Ok(written)
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

                    let tasks = env.tasks().clone();
                    let written = wasi_try_ok!(__asyncify_light(env, None, async move {
                        let mut sent = 0usize;
                        for iovs in iovs_arr.iter() {
                            let buf = WasmPtr::<u8, M>::new(iovs.buf)
                                .slice(&memory, iovs.buf_len)
                                .map_err(mem_error_to_wasi)?
                                .access()
                                .map_err(mem_error_to_wasi)?;
                            let local_sent =
                                socket.send(tasks.deref(), buf.as_ref(), fd_flags).await?;
                            sent += local_sent;
                            if local_sent != buf.len() {
                                break;
                            }
                        }
                        Ok(sent)
                    })?);
                    (written, false)
                }
                Kind::Pipe { pipe } => {
                    let mut written = 0usize;
                    for iovs in iovs_arr.iter() {
                        let buf = wasi_try_ok!(WasmPtr::<u8, M>::new(iovs.buf)
                            .slice(&memory, iovs.buf_len)
                            .map_err(mem_error_to_wasi));
                        let buf = wasi_try_ok!(buf.access().map_err(mem_error_to_wasi));
                        let local_written = wasi_try_ok!(
                            std::io::Write::write(pipe, buf.as_ref()).map_err(map_io_err)
                        );
                        written += local_written;
                        if local_written != buf.len() {
                            break;
                        }
                    }
                    (written, false)
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(Errno::Isdir);
                }
                Kind::EventNotifications(inner) => {
                    let mut written = 0usize;
                    for iovs in iovs_arr.iter() {
                        let buf_len: usize =
                            wasi_try_ok!(iovs.buf_len.try_into().map_err(|_| Errno::Inval));
                        let will_be_written = buf_len;

                        let val_cnt = buf_len / std::mem::size_of::<u64>();
                        let val_cnt: M::Offset =
                            wasi_try_ok!(val_cnt.try_into().map_err(|_| Errno::Inval));

                        let vals = wasi_try_ok!(WasmPtr::<u64, M>::new(iovs.buf)
                            .slice(&memory, val_cnt as M::Offset)
                            .map_err(mem_error_to_wasi));
                        let vals = wasi_try_ok!(vals.access().map_err(mem_error_to_wasi));
                        for val in vals.iter() {
                            inner.write(*val);
                        }

                        written += will_be_written;
                    }
                    (written, false)
                }
                Kind::Symlink { .. } => return Ok(Errno::Inval),
                Kind::Buffer { buffer } => {
                    let mut written = 0usize;
                    for iovs in iovs_arr.iter() {
                        let buf = wasi_try_ok!(WasmPtr::<u8, M>::new(iovs.buf)
                            .slice(&memory, iovs.buf_len)
                            .map_err(mem_error_to_wasi));
                        let buf = wasi_try_ok!(buf.access().map_err(mem_error_to_wasi));
                        let local_written =
                            wasi_try_ok!(
                                std::io::Write::write(buffer, buf.as_ref()).map_err(map_io_err)
                            );
                        written += local_written;
                        if local_written != buf.len() {
                            break;
                        }
                    }
                    (written, false)
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
    Span::current().record("nwritten", bytes_written);

    let memory = env.memory_view(&ctx);
    let nwritten_ref = nwritten.deref(&memory);
    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(nwritten_ref.write(bytes_written));

    Ok(Errno::Success)
}
