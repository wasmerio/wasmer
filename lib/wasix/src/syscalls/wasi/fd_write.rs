use std::task::Waker;

use super::*;
use crate::{fs::NotificationInner, net::socket::TimeType, syscalls::*};
#[cfg(feature = "journal")]
use crate::{
    journal::{JournalEffector, JournalEntry},
    utils::map_snapshot_err,
};

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
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let enable_journal = env.enable_journal;
    let fd_entry = {
        let state = env.state.clone();
        wasi_try_ok!(state.fs.get_fd(fd))
    };
    let offset = fd_entry.inner.offset.load(Ordering::Acquire) as usize;

    let bytes_written = wasi_try_ok!(fd_write_internal::<M>(
        &mut ctx,
        fd,
        fd_entry,
        FdWriteSource::Iovs { iovs, iovs_len },
        offset as u64,
        true,
        enable_journal,
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
    WasiEnv::do_pending_operations(&mut ctx)?;

    let enable_snapshot_capture = ctx.data().enable_journal;

    let fd_entry = {
        let env = ctx.data();
        let state = env.state.clone();
        wasi_try_ok!(state.fs.get_fd(fd))
    };
    let bytes_written = wasi_try_ok!(fd_write_internal::<M>(
        &mut ctx,
        fd,
        fd_entry,
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

// ── Write outcome ───────────────────────────────────────────────────────────

struct WriteOutcome {
    bytes_written: usize,
    is_file: bool,
    can_snapshot: bool,
}

// ── Per-Kind write helpers ──────────────────────────────────────────────────
//
// Each significant Kind arm is extracted into its own function. The main
// `fd_write_internal` match becomes a thin dispatcher.

/// Write to a `Kind::File` handle.
///
/// The inode guard must already be dropped before calling this so that
/// `__asyncify_light` can run without holding the write lock.
#[allow(clippy::await_holding_lock)]
fn write_to_file<M: MemorySize>(
    env: &WasiEnv,
    fd_entry: &Fd,
    data: &FdWriteSource<'_, M>,
    memory: &MemoryView,
    offset: &mut u64,
    is_stdio: bool,
    handle: Arc<std::sync::RwLock<Box<dyn VirtualFile + Send + Sync + 'static>>>,
) -> Result<Result<usize, Errno>, WasiError> {
    let fd_flags = fd_entry.inner.flags;
    let res = __asyncify_light(
        env,
        if fd_flags.contains(Fdflags::NONBLOCK) {
            Some(Duration::ZERO)
        } else {
            None
        },
        async {
            let mut handle = handle.write().unwrap();
            if !is_stdio {
                if fd_flags.contains(Fdflags::APPEND) {
                    *offset = fd_entry.inode.stat.read().unwrap().st_size;
                    fd_entry.inner.offset.store(*offset, Ordering::Release);
                }
                handle
                    .seek(std::io::SeekFrom::Start(*offset))
                    .await
                    .map_err(map_io_err)?;
            }

            let mut written = 0usize;
            match data {
                FdWriteSource::Iovs { iovs, iovs_len } => {
                    let iovs_arr = iovs.slice(memory, *iovs_len).map_err(mem_error_to_wasi)?;
                    let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
                    for iov in iovs_arr.iter() {
                        let buf = WasmPtr::<u8, M>::new(iov.buf)
                            .slice(memory, iov.buf_len)
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

    Ok(res?.map_err(|err| match err {
        Errno::Timedout => Errno::Again,
        a => a,
    }))
}

/// Write to a `Kind::Socket`.
///
/// The inode guard must already be dropped before calling this.
fn write_to_socket<M: MemorySize>(
    env: &WasiEnv,
    data: &FdWriteSource<'_, M>,
    memory: &MemoryView,
    fd_flags: Fdflags,
    socket: InodeSocket,
) -> Result<Result<usize, Errno>, WasiError> {
    let nonblocking = fd_flags.contains(Fdflags::NONBLOCK);
    let timeout = socket
        .opt_time(TimeType::WriteTimeout)
        .ok()
        .flatten()
        .unwrap_or(Duration::from_secs(30));
    let tasks = env.tasks().clone();

    let res = __asyncify_light(env, None, async {
        let mut sent = 0usize;
        match data {
            FdWriteSource::Iovs { iovs, iovs_len } => {
                let iovs_arr = iovs.slice(memory, *iovs_len).map_err(mem_error_to_wasi)?;
                let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
                for iov in iovs_arr.iter() {
                    let buf = WasmPtr::<u8, M>::new(iov.buf)
                        .slice(memory, iov.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    let local_sent = socket
                        .send(tasks.deref(), buf.as_ref(), Some(timeout), nonblocking)
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

    Ok(res?)
}

/// Result of writing to a pipe-like fd. `BrokenPipe` is returned instead of an
/// error so the caller can raise SIGPIPE after releasing iovec borrows.
enum PipeWriteResult {
    Ok(usize),
    BrokenPipe,
}

/// Write to a pipe-like writer (`PipeTx` or `Pipe`/`DuplexPipe`).
///
/// Both implement `std::io::Write` with identical semantics. The inode guard
/// is still held by the caller — this function only touches the writer.
fn write_to_pipe<M: MemorySize>(
    writer: &mut dyn std::io::Write,
    data: &FdWriteSource<'_, M>,
    memory: &MemoryView,
) -> Result<PipeWriteResult, Errno> {
    match data {
        FdWriteSource::Iovs { iovs, iovs_len } => {
            let iovs_arr = iovs.slice(memory, *iovs_len).map_err(mem_error_to_wasi)?;
            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
            let mut written = 0usize;
            let mut broken = false;
            for iov in iovs_arr.iter() {
                let buf = WasmPtr::<u8, M>::new(iov.buf)
                    .slice(memory, iov.buf_len)
                    .map_err(mem_error_to_wasi)?
                    .access()
                    .map_err(mem_error_to_wasi)?;
                match std::io::Write::write(writer, buf.as_ref()) {
                    Ok(n) => {
                        written += n;
                        if n != buf.len() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                        broken = true;
                        break;
                    }
                    Err(e) => return Err(map_io_err(e)),
                }
            }
            // iovs_arr is dropped here, releasing the memory borrow before
            // the caller raises SIGPIPE (which needs ctx).
            drop(iovs_arr);
            if broken {
                Ok(PipeWriteResult::BrokenPipe)
            } else {
                Ok(PipeWriteResult::Ok(written))
            }
        }
        FdWriteSource::Buffer(data) => match std::io::Write::write_all(writer, data) {
            Ok(()) => Ok(PipeWriteResult::Ok(data.len())),
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(PipeWriteResult::BrokenPipe),
            Err(e) => Err(map_io_err(e)),
        },
    }
}

/// Write u64 values to an `EventNotifications` fd.
fn write_to_event_notifications<M: MemorySize>(
    inner: &NotificationInner,
    data: &FdWriteSource<'_, M>,
    memory: &MemoryView,
) -> Result<usize, Errno> {
    match data {
        FdWriteSource::Iovs { iovs, iovs_len } => {
            let iovs_arr = iovs.slice(memory, *iovs_len).map_err(mem_error_to_wasi)?;
            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
            let mut written = 0usize;
            for iov in iovs_arr.iter() {
                let buf_len: usize = iov.buf_len.try_into().map_err(|_| Errno::Inval)?;
                let val_cnt = buf_len / std::mem::size_of::<u64>();
                let val_cnt: M::Offset = val_cnt.try_into().map_err(|_| Errno::Inval)?;
                let vals = WasmPtr::<u64, M>::new(iov.buf)
                    .slice(memory, val_cnt as M::Offset)
                    .map_err(mem_error_to_wasi)?;
                let vals = vals.access().map_err(mem_error_to_wasi)?;
                for val in vals.iter() {
                    inner.write(*val);
                }
                written += buf_len;
            }
            Ok(written)
        }
        FdWriteSource::Buffer(data) => {
            let mut written = 0usize;
            let cnt = data.len() / std::mem::size_of::<u64>();
            for n in 0..cnt {
                let start = n * std::mem::size_of::<u64>();
                let bytes = [
                    data[start],
                    data[start + 1],
                    data[start + 2],
                    data[start + 3],
                    data[start + 4],
                    data[start + 5],
                    data[start + 6],
                    data[start + 7],
                ];
                inner.write(u64::from_ne_bytes(bytes));
                written += std::mem::size_of::<u64>();
            }
            Ok(written)
        }
    }
}

/// Write to an in-memory `Kind::Buffer`.
fn write_to_buffer<M: MemorySize>(
    buffer: &mut Vec<u8>,
    data: &FdWriteSource<'_, M>,
    memory: &MemoryView,
) -> Result<usize, Errno> {
    match data {
        FdWriteSource::Iovs { iovs, iovs_len } => {
            let iovs_arr = iovs.slice(memory, *iovs_len).map_err(mem_error_to_wasi)?;
            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
            let mut written = 0usize;
            for iov in iovs_arr.iter() {
                let buf = WasmPtr::<u8, M>::new(iov.buf)
                    .slice(memory, iov.buf_len)
                    .map_err(mem_error_to_wasi)?
                    .access()
                    .map_err(mem_error_to_wasi)?;
                let local_written =
                    std::io::Write::write(buffer, buf.as_ref()).map_err(map_io_err)?;
                written += local_written;
                if local_written != buf.len() {
                    break;
                }
            }
            Ok(written)
        }
        FdWriteSource::Buffer(data) => {
            std::io::Write::write_all(buffer, data).map_err(map_io_err)?;
            Ok(data.len())
        }
    }
}

// ── Main dispatch ───────────────────────────────────────────────────────────

#[allow(clippy::await_holding_lock)]
pub(crate) fn fd_write_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    fd_entry: Fd,
    data: FdWriteSource<'_, M>,
    offset: u64,
    should_update_cursor: bool,
    should_snapshot: bool,
) -> Result<Result<usize, Errno>, WasiError> {
    let mut offset = offset;
    let mut env = ctx.data();
    let is_stdio = fd_entry.is_stdio;

    if !is_stdio && !fd_entry.inner.rights.contains(Rights::FD_WRITE) {
        return Ok(Err(Errno::Access));
    }

    let fd_flags = fd_entry.inner.flags;
    let mut memory = unsafe { env.memory_view(&ctx) };

    let outcome = {
        let (memory, _) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
        let mut inode = fd_entry.inode.write();
        match inode.deref_mut() {
            Kind::File { handle, .. } => {
                let Some(handle) = handle else {
                    return Ok(Err(Errno::Inval));
                };
                let handle = handle.clone();
                drop(inode);

                let written = wasi_try_ok_ok!(write_to_file::<M>(
                    env,
                    &fd_entry,
                    &data,
                    &memory,
                    &mut offset,
                    is_stdio,
                    handle
                )?);
                WriteOutcome {
                    bytes_written: written,
                    is_file: true,
                    can_snapshot: true,
                }
            }

            Kind::Socket { socket } => {
                let socket = socket.clone();
                drop(inode);

                let written =
                    wasi_try_ok_ok!(write_to_socket::<M>(env, &data, &memory, fd_flags, socket)?);
                WriteOutcome {
                    bytes_written: written,
                    is_file: false,
                    can_snapshot: false,
                }
            }

            Kind::PipeRx { .. } => return Ok(Err(Errno::Badf)),

            Kind::PipeTx { tx } => {
                let result = wasi_try_ok_ok!(write_to_pipe::<M>(tx, &data, &memory));
                match result {
                    PipeWriteResult::Ok(written) => WriteOutcome {
                        bytes_written: written,
                        is_file: false,
                        can_snapshot: true,
                    },
                    PipeWriteResult::BrokenPipe => {
                        env.process.signal_process(Signal::Sigpipe);
                        wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);
                        return Ok(Err(Errno::Pipe));
                    }
                }
            }

            Kind::DuplexPipe { pipe } => {
                let result = wasi_try_ok_ok!(write_to_pipe::<M>(pipe, &data, &memory));
                match result {
                    PipeWriteResult::Ok(written) => WriteOutcome {
                        bytes_written: written,
                        is_file: false,
                        can_snapshot: true,
                    },
                    PipeWriteResult::BrokenPipe => {
                        env.process.signal_process(Signal::Sigpipe);
                        wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);
                        return Ok(Err(Errno::Pipe));
                    }
                }
            }

            Kind::Dir { .. } | Kind::Root { .. } => return Ok(Err(Errno::Isdir)),
            Kind::Symlink { .. } | Kind::Epoll { .. } => return Ok(Err(Errno::Inval)),

            Kind::EventNotifications { inner } => {
                let written =
                    wasi_try_ok_ok!(write_to_event_notifications::<M>(inner, &data, &memory));
                WriteOutcome {
                    bytes_written: written,
                    is_file: false,
                    can_snapshot: true,
                }
            }

            Kind::Buffer { buffer } => {
                let written = wasi_try_ok_ok!(write_to_buffer::<M>(buffer, &data, &memory));
                WriteOutcome {
                    bytes_written: written,
                    is_file: false,
                    can_snapshot: true,
                }
            }
        }
    };

    let WriteOutcome {
        bytes_written,
        is_file,
        can_snapshot,
    } = outcome;

    // Journal snapshot
    #[cfg(feature = "journal")]
    if should_snapshot
        && can_snapshot
        && bytes_written > 0
        && let FdWriteSource::Iovs { iovs, iovs_len } = data
    {
        JournalEffector::save_fd_write(ctx, fd, offset, bytes_written, iovs, iovs_len).map_err(
            |err| {
                tracing::error!("failed to save terminal data - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            },
        )?;
    }

    env = ctx.data();
    memory = unsafe { env.memory_view(&ctx) };

    // For stdio we don't need to update the offset or file size, we can just return
    if is_stdio {
        return Ok(Ok(bytes_written));
    }

    // If it is not stdio, we need to update the offset and file size accordingly.

    // Update cursor and stat
    let curr_offset = if is_file && should_update_cursor {
        let bytes_written = bytes_written as u64;
        fd_entry
            .inner
            .offset
            .fetch_add(bytes_written, Ordering::AcqRel)
            // fetch_add returns the previous value, we have to add bytes_written again here
            + bytes_written
    } else {
        fd_entry.inner.offset.load(Ordering::Acquire)
    };

    // we set the size but we don't return any errors if it fails as
    // pipes and sockets will not do anything with this
    let (mut memory, _, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    if is_file {
        let mut stat = fd_entry.inode.stat.write().unwrap();
        if should_update_cursor {
            // If we wrote before the end, the current size is still correct.
            // Otherwise, we only got as far as the current cursor. So, the
            // max of the two is the correct new size.
            stat.st_size = stat.st_size.max(curr_offset);
        } else {
            // pwrite does not update the cursor of the file so to calculate the final
            // size of the file we compute where the cursor would have been if it was updated,
            // and get the max value between it and the current size.
            stat.st_size = stat.st_size.max(offset + bytes_written as u64);
        }
    } else {
        // Cast is valid because we don't support 128 bit systems...
        fd_entry.inode.stat.write().unwrap().st_size += bytes_written as u64;
    }

    Ok(Ok(bytes_written))
}
