use std::task::Waker;

use super::*;
#[cfg(feature = "journal")]
use crate::{
    journal::{JournalEffector, JournalEntry},
    utils::map_snapshot_err,
};
use crate::{net::socket::TimeType, syscalls::*};

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
#[instrument(level = "trace", skip_all, fields(%fd, nwritten = field::Empty), ret)]
pub fn fd_write<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let env = ctx.data();
    let offset = {
        let state = env.state.clone();
        let inodes = state.inodes.clone();

        let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));
        fd_entry.offset.load(Ordering::Acquire) as usize
    };

    let bytes_written = wasi_try_ok!(fd_write_internal::<M>(
        &ctx,
        fd,
        FdWriteSource::Iovs { iovs, iovs_len },
        offset as u64,
        true,
        env.enable_journal,
    )?);

    Span::current().record("nwritten", bytes_written);

    let mut env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let nwritten_ref = nwritten.deref(&memory);
    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(nwritten_ref.write(bytes_written));

    Ok(Errno::Success)
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
#[instrument(level = "trace", skip_all, fields(%fd, %offset, nwritten = field::Empty), ret)]
pub fn fd_pwrite<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
    iovs_len: M::Offset,
    offset: Filesize,
    nwritten: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let enable_snapshot_capture = ctx.data().enable_journal;

    let bytes_written = wasi_try_ok!(fd_write_internal::<M>(
        &ctx,
        fd,
        FdWriteSource::Iovs { iovs, iovs_len },
        offset,
        false,
        enable_snapshot_capture,
    )?);

    Span::current().record("nwritten", bytes_written);

    let mut env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let nwritten_ref = nwritten.deref(&memory);
    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(nwritten_ref.write(bytes_written));

    Ok(Errno::Success)
}

pub(crate) enum FdWriteSource<'a, M: MemorySize> {
    Iovs {
        iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
        iovs_len: M::Offset,
    },
    Buffer(Cow<'a, [u8]>),
}

pub(crate) fn fd_write_internal<M: MemorySize>(
    ctx: &FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    data: FdWriteSource<'_, M>,
    offset: u64,
    should_update_cursor: bool,
    should_snapshot: bool,
) -> Result<Result<usize, Errno>, WasiError> {
    let mut env = ctx.data();
    let state = env.state.clone();

    let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(fd));
    let is_stdio = fd_entry.is_stdio;

    let bytes_written = {
        if !is_stdio && !fd_entry.rights.contains(Rights::FD_WRITE) {
            return Ok(Err(Errno::Access));
        }

        let fd_flags = fd_entry.flags;
        let mut memory = unsafe { env.memory_view(&ctx) };

        let (bytes_written, can_update_cursor, can_snapshot) = {
            let (mut memory, _) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
            let mut guard = fd_entry.inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let handle = handle.clone();
                        drop(guard);

                        let res = __asyncify_light(
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
                                        .seek(std::io::SeekFrom::Start(offset))
                                        .await
                                        .map_err(map_io_err)?;
                                }

                                let mut written = 0usize;

                                match &data {
                                    FdWriteSource::Iovs { iovs, iovs_len } => {
                                        let iovs_arr = iovs
                                            .slice(&memory, *iovs_len)
                                            .map_err(mem_error_to_wasi)?;
                                        let iovs_arr =
                                            iovs_arr.access().map_err(mem_error_to_wasi)?;
                                        for iovs in iovs_arr.iter() {
                                            let buf = WasmPtr::<u8, M>::new(iovs.buf)
                                                .slice(&memory, iovs.buf_len)
                                                .map_err(mem_error_to_wasi)?
                                                .access()
                                                .map_err(mem_error_to_wasi)?;
                                            let local_written =
                                                match handle.write(buf.as_ref()).await {
                                                    Ok(s) => s,
                                                    Err(_) if written > 0 => break,
                                                    Err(err) => return Err(map_io_err(err)),
                                                };
                                            written += local_written;
                                            if local_written != buf.len() {
                                                break;
                                            }
                                        }
                                    }
                                    FdWriteSource::Buffer(data) => {
                                        handle.write_all(data).await?;
                                        written += data.len();
                                    }
                                }

                                if is_stdio {
                                    handle.flush().await.map_err(map_io_err)?;
                                }
                                Ok(written)
                            },
                        );
                        let written = wasi_try_ok_ok!(res?.map_err(|err| match err {
                            Errno::Timedout => Errno::Again,
                            a => a,
                        }));

                        (written, true, true)
                    } else {
                        return Ok(Err(Errno::Inval));
                    }
                }
                Kind::Socket { socket } => {
                    let socket = socket.clone();
                    drop(guard);

                    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);
                    let timeout = socket
                        .opt_time(TimeType::WriteTimeout)
                        .ok()
                        .flatten()
                        .unwrap_or(Duration::from_secs(30));

                    let tasks = env.tasks().clone();

                    let res = __asyncify_light(env, None, async {
                        let mut sent = 0usize;

                        match &data {
                            FdWriteSource::Iovs { iovs, iovs_len } => {
                                let iovs_arr =
                                    iovs.slice(&memory, *iovs_len).map_err(mem_error_to_wasi)?;
                                let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
                                for iovs in iovs_arr.iter() {
                                    let buf = WasmPtr::<u8, M>::new(iovs.buf)
                                        .slice(&memory, iovs.buf_len)
                                        .map_err(mem_error_to_wasi)?
                                        .access()
                                        .map_err(mem_error_to_wasi)?;
                                    let local_sent = socket
                                        .send(
                                            tasks.deref(),
                                            buf.as_ref(),
                                            Some(timeout),
                                            nonblocking,
                                        )
                                        .await?;
                                    sent += local_sent;
                                    if local_sent != buf.len() {
                                        break;
                                    }
                                }
                            }
                            FdWriteSource::Buffer(data) => {
                                sent += socket
                                    .send(tasks.deref(), data.as_ref(), Some(timeout), nonblocking)
                                    .await?;
                            }
                        }
                        Ok(sent)
                    });
                    let written = wasi_try_ok_ok!(res?);
                    (written, false, false)
                }
                Kind::Pipe { pipe } => {
                    let mut written = 0usize;

                    match &data {
                        FdWriteSource::Iovs { iovs, iovs_len } => {
                            let iovs_arr = wasi_try_ok_ok!(iovs
                                .slice(&memory, *iovs_len)
                                .map_err(mem_error_to_wasi));
                            let iovs_arr =
                                wasi_try_ok_ok!(iovs_arr.access().map_err(mem_error_to_wasi));
                            for iovs in iovs_arr.iter() {
                                let buf = wasi_try_ok_ok!(WasmPtr::<u8, M>::new(iovs.buf)
                                    .slice(&memory, iovs.buf_len)
                                    .map_err(mem_error_to_wasi));
                                let buf = wasi_try_ok_ok!(buf.access().map_err(mem_error_to_wasi));
                                let local_written =
                                    wasi_try_ok_ok!(std::io::Write::write(pipe, buf.as_ref())
                                        .map_err(map_io_err));

                                written += local_written;
                                if local_written != buf.len() {
                                    break;
                                }
                            }
                        }
                        FdWriteSource::Buffer(data) => {
                            wasi_try_ok_ok!(
                                std::io::Write::write_all(pipe, data).map_err(map_io_err)
                            );
                            written += data.len();
                        }
                    }

                    (written, false, true)
                }
                Kind::Dir { .. } | Kind::Root { .. } => {
                    // TODO: verify
                    return Ok(Err(Errno::Isdir));
                }
                Kind::EventNotifications { inner } => {
                    let mut written = 0usize;

                    match &data {
                        FdWriteSource::Iovs { iovs, iovs_len } => {
                            let iovs_arr = wasi_try_ok_ok!(iovs
                                .slice(&memory, *iovs_len)
                                .map_err(mem_error_to_wasi));
                            let iovs_arr =
                                wasi_try_ok_ok!(iovs_arr.access().map_err(mem_error_to_wasi));
                            for iovs in iovs_arr.iter() {
                                let buf_len: usize = wasi_try_ok_ok!(iovs
                                    .buf_len
                                    .try_into()
                                    .map_err(|_| Errno::Inval));
                                let will_be_written = buf_len;

                                let val_cnt = buf_len / std::mem::size_of::<u64>();
                                let val_cnt: M::Offset =
                                    wasi_try_ok_ok!(val_cnt.try_into().map_err(|_| Errno::Inval));

                                let vals = wasi_try_ok_ok!(WasmPtr::<u64, M>::new(iovs.buf)
                                    .slice(&memory, val_cnt as M::Offset)
                                    .map_err(mem_error_to_wasi));
                                let vals =
                                    wasi_try_ok_ok!(vals.access().map_err(mem_error_to_wasi));
                                for val in vals.iter() {
                                    inner.write(*val);
                                }

                                written += will_be_written;
                            }
                        }
                        FdWriteSource::Buffer(data) => {
                            let cnt = data.len() / std::mem::size_of::<u64>();
                            for n in 0..cnt {
                                let start = n * std::mem::size_of::<u64>();
                                let data = [
                                    data[start],
                                    data[start + 1],
                                    data[start + 2],
                                    data[start + 3],
                                    data[start + 4],
                                    data[start + 5],
                                    data[start + 6],
                                    data[start + 7],
                                ];
                                inner.write(u64::from_ne_bytes(data));
                            }
                        }
                    }

                    (written, false, true)
                }
                Kind::Symlink { .. } | Kind::Epoll { .. } => return Ok(Err(Errno::Inval)),
                Kind::Buffer { buffer } => {
                    let mut written = 0usize;

                    match &data {
                        FdWriteSource::Iovs { iovs, iovs_len } => {
                            let iovs_arr = wasi_try_ok_ok!(iovs
                                .slice(&memory, *iovs_len)
                                .map_err(mem_error_to_wasi));
                            let iovs_arr =
                                wasi_try_ok_ok!(iovs_arr.access().map_err(mem_error_to_wasi));
                            for iovs in iovs_arr.iter() {
                                let buf = wasi_try_ok_ok!(WasmPtr::<u8, M>::new(iovs.buf)
                                    .slice(&memory, iovs.buf_len)
                                    .map_err(mem_error_to_wasi));
                                let buf = wasi_try_ok_ok!(buf.access().map_err(mem_error_to_wasi));
                                let local_written =
                                    wasi_try_ok_ok!(std::io::Write::write(buffer, buf.as_ref())
                                        .map_err(map_io_err));
                                written += local_written;
                                if local_written != buf.len() {
                                    break;
                                }
                            }
                        }
                        FdWriteSource::Buffer(data) => {
                            wasi_try_ok_ok!(
                                std::io::Write::write_all(buffer, data).map_err(map_io_err)
                            );
                            written += data.len();
                        }
                    }

                    (written, false, true)
                }
            }
        };

        #[cfg(feature = "journal")]
        if should_snapshot && can_snapshot && bytes_written > 0 {
            if let FdWriteSource::Iovs { iovs, iovs_len } = data {
                JournalEffector::save_fd_write(ctx, fd, offset, bytes_written, iovs, iovs_len)
                    .map_err(|err| {
                        tracing::error!("failed to save terminal data - {}", err);
                        WasiError::Exit(ExitCode::Errno(Errno::Fault))
                    })?;
            }
        }

        env = ctx.data();
        memory = unsafe { env.memory_view(&ctx) };

        // reborrow and update the size
        if !is_stdio {
            if can_update_cursor && should_update_cursor {
                let mut fd_map = state.fs.fd_map.write().unwrap();
                let fd_entry = wasi_try_ok_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
                fd_entry
                    .offset
                    .fetch_add(bytes_written as u64, Ordering::AcqRel);
            }

            // we set the size but we don't return any errors if it fails as
            // pipes and sockets will not do anything with this
            let (mut memory, _, inodes) =
                unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
            // Cast is valid because we don't support 128 bit systems...
            fd_entry.inode.stat.write().unwrap().st_size += bytes_written as u64;
        }
        bytes_written
    };

    Ok(Ok(bytes_written))
}
