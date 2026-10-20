use std::task::Waker;

use super::*;
#[cfg(feature = "journal")]
use crate::{
    journal::{JournalEffector, JournalEntry},
    utils::map_snapshot_err,
};
use crate::{net::MAX_SOCKET_PAYLOAD, net::socket::TimeType, syscalls::*};

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

/// Cap on how much of a `writev` one coalescing buffer holds. The guest
/// declares the iovec lengths and nothing has validated the matching pointers
/// yet, so the buffer is sized by this rather than by what the guest claims.
/// It also keeps a coalesced write clear of the 8 MiB frame limit that
/// `LengthDelimitedCodec` imposes on remote virtual-net sockets.
pub(crate) const MAX_STREAM_COALESCE: usize = 1024 * 1024;

const _: () = assert!(MAX_STREAM_COALESCE < 8 * 1024 * 1024);
/// A datagram is refused past `MAX_SOCKET_PAYLOAD`, so it never reaches the
/// bounded gather that streams use.
const _: () = assert!(MAX_SOCKET_PAYLOAD < MAX_STREAM_COALESCE);

impl<'a, M: MemorySize> FdWriteSource<'a, M> {
    pub(crate) fn coalesce(
        &self,
        memory: &MemoryView,
        max_len: usize,
    ) -> Result<Cow<'a, [u8]>, Errno> {
        match self {
            FdWriteSource::Iovs { iovs, iovs_len } => {
                let iovs_arr = iovs.slice(memory, *iovs_len).map_err(mem_error_to_wasi)?;
                let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;

                let mut total_len = 0usize;
                for iov in iovs_arr.iter() {
                    let len = iov.buf_len.into() as usize;
                    total_len = total_len.checked_add(len).ok_or(Errno::Msgsize)?;
                }
                if total_len > max_len {
                    return Err(Errno::Msgsize);
                }

                let mut coalesced = Vec::new();
                coalesced
                    .try_reserve_exact(total_len)
                    .map_err(|_| Errno::Nomem)?;

                for iov in iovs_arr.iter() {
                    let buf = WasmPtr::<u8, M>::new(iov.buf)
                        .slice(memory, iov.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    coalesced.extend_from_slice(buf.as_ref());
                }

                Ok(Cow::Owned(coalesced))
            }
            FdWriteSource::Buffer(cow) => {
                if cow.len() > max_len {
                    return Err(Errno::Msgsize);
                }
                Ok(cow.clone())
            }
        }
    }

    /// Gathers at most `limit` bytes starting `skip` bytes into the iovecs,
    /// treating them as one stream. Unlike [`Self::coalesce`] this never sizes
    /// an allocation by a guest-declared length, so a caller can walk a large
    /// `writev` in bounded steps instead of refusing it.
    pub(crate) fn coalesce_from(
        &self,
        memory: &MemoryView,
        skip: usize,
        limit: usize,
    ) -> Result<Cow<'a, [u8]>, Errno> {
        match self {
            FdWriteSource::Iovs { iovs, iovs_len } => {
                let iovs_arr = iovs.slice(memory, *iovs_len).map_err(mem_error_to_wasi)?;
                let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;

                let mut coalesced: Vec<u8> = Vec::new();
                let mut consumed = 0usize;

                for iov in iovs_arr.iter() {
                    let room = limit - coalesced.len();
                    if room == 0 {
                        break;
                    }

                    let iov_start = consumed;
                    let len = iov.buf_len.into() as usize;
                    consumed = consumed.checked_add(len).ok_or(Errno::Msgsize)?;
                    if consumed <= skip {
                        continue;
                    }

                    let buf = WasmPtr::<u8, M>::new(iov.buf)
                        .slice(memory, iov.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    let buf = buf.as_ref();

                    let start = skip.saturating_sub(iov_start);
                    if start >= buf.len() {
                        continue;
                    }
                    let take = room.min(buf.len() - start);
                    coalesced.try_reserve(take).map_err(|_| Errno::Nomem)?;
                    coalesced.extend_from_slice(&buf[start..start + take]);
                }

                Ok(Cow::Owned(coalesced))
            }
            FdWriteSource::Buffer(cow) => {
                let start = skip.min(cow.len());
                let end = start.saturating_add(limit).min(cow.len());
                Ok(Cow::Owned(cow[start..end].to_vec()))
            }
        }
    }
}

#[allow(clippy::await_holding_lock)]
pub(crate) fn fd_write_internal<M: MemorySize>(
    mut ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    fd_entry: Fd,
    data: FdWriteSource<'_, M>,
    offset: u64,
    should_update_cursor: bool,
    should_snapshot: bool,
) -> Result<Result<usize, Errno>, WasiError> {
    let mut offset = offset;
    let mut env = ctx.data();
    let state = env.state.clone();
    let is_stdio = fd_entry.is_stdio;

    let bytes_written = {
        if !is_stdio && !fd_entry.inner.rights.contains(Rights::FD_WRITE) {
            return Ok(Err(Errno::Access));
        }

        let fd_flags = fd_entry.inner.flags;
        let mut memory = unsafe { env.memory_view(&ctx) };

        let (bytes_written, is_file, can_snapshot) = {
            let (mut memory, _) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
            let mut guard = fd_entry.inode.write();
            match guard.deref_mut() {
                Kind::File { handle, .. } => {
                    if let Some(handle) = handle {
                        let handle = handle.clone();
                        drop(guard);

                        let res = __asyncify_light(
                            env,
                            if fd_entry.inner.flags.contains(Fdflags::NONBLOCK) {
                                Some(Duration::ZERO)
                            } else {
                                None
                            },
                            async {
                                let mut handle = handle.write().unwrap();
                                if !is_stdio {
                                    if fd_entry.inner.flags.contains(Fdflags::APPEND) {
                                        // `fdflags::append` means we need to seek to the end before writing.
                                        offset = fd_entry.inode.stat.read().unwrap().st_size;
                                        fd_entry.inner.offset.store(offset, Ordering::Release);
                                    }

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

                        // VirtualConnectedSocket exposes one contiguous send operation. Preserve
                        // writev's single-operation ordering by coalescing guest iovecs before
                        // crossing that boundary; issuing one send per iovec lets the peer react
                        // between logically adjacent protocol frames.
                        if socket.is_dgram() {
                            let data = data.coalesce(&memory, MAX_SOCKET_PAYLOAD)?;
                            sent += socket
                                .send(tasks.deref(), data.as_ref(), Some(timeout), nonblocking)
                                .await?;
                            return Ok(sent);
                        }

                        // One send per chunk keeps writev's ordering for
                        // anything that fits in a chunk, while the bound keeps
                        // a guest-declared length from sizing the buffer. The
                        // loop leaves the returned count to the socket, as a
                        // single unbounded send did.
                        loop {
                            let chunk = data.coalesce_from(&memory, sent, MAX_STREAM_COALESCE)?;
                            if chunk.is_empty() {
                                break;
                            }
                            let chunk_len = chunk.len();
                            let local_sent = match socket
                                .send(tasks.deref(), chunk.as_ref(), Some(timeout), nonblocking)
                                .await
                            {
                                Ok(local_sent) => local_sent,
                                // Report the progress already made, as a write
                                // that fails part way through does.
                                Err(_) if sent > 0 => break,
                                Err(err) => return Err(err),
                            };
                            sent += local_sent;

                            if local_sent != chunk_len {
                                // A blocking write runs to completion on Unix,
                                // waiting for room rather than reporting a
                                // short count, so keep pushing the remainder.
                                // A non-blocking one reports what it managed.
                                // `local_sent == 0` would not make progress.
                                if nonblocking || local_sent == 0 {
                                    break;
                                }
                            }
                        }
                        Ok(sent)
                    });
                    let written = wasi_try_ok_ok!(res?);
                    (written, false, false)
                }
                Kind::PipeRx { .. } => {
                    return Ok(Err(Errno::Badf));
                }
                Kind::PipeTx { tx } => {
                    let mut written = 0usize;

                    match &data {
                        FdWriteSource::Iovs { iovs, iovs_len } => {
                            let mut raise_sigpipe = false;
                            let iovs_arr = wasi_try_ok_ok!(
                                iovs.slice(&memory, *iovs_len).map_err(mem_error_to_wasi)
                            );
                            let iovs_arr =
                                wasi_try_ok_ok!(iovs_arr.access().map_err(mem_error_to_wasi));
                            for iovs in iovs_arr.iter() {
                                let buf = wasi_try_ok_ok!(
                                    WasmPtr::<u8, M>::new(iovs.buf)
                                        .slice(&memory, iovs.buf_len)
                                        .map_err(mem_error_to_wasi)
                                );
                                let buf = wasi_try_ok_ok!(buf.access().map_err(mem_error_to_wasi));
                                let write_result = std::io::Write::write(tx, buf.as_ref());
                                let local_written = match write_result {
                                    Ok(w) => w,
                                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                                        // Need to do this to avoid double borrow on ctx with iovs_arr
                                        raise_sigpipe = true;
                                        break;
                                    }
                                    Err(e) => return Ok(Err(map_io_err(e))),
                                };

                                written += local_written;
                                if local_written != buf.len() {
                                    break;
                                }
                            }

                            drop(iovs_arr);

                            if raise_sigpipe {
                                env.process.signal_process(Signal::Sigpipe);
                                wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);
                                return Ok(Err(Errno::Pipe));
                            }
                        }
                        FdWriteSource::Buffer(data) => {
                            match std::io::Write::write_all(tx, data) {
                                Ok(()) => (),
                                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                                    env.process.signal_process(Signal::Sigpipe);
                                    wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);
                                    return Ok(Err(Errno::Pipe));
                                }
                                Err(e) => return Ok(Err(map_io_err(e))),
                            };
                            written += data.len();
                        }
                    }

                    (written, false, true)
                }
                Kind::DuplexPipe { pipe } => {
                    let mut written = 0usize;

                    match &data {
                        FdWriteSource::Iovs { iovs, iovs_len } => {
                            let mut raise_sigpipe = false;
                            let iovs_arr = wasi_try_ok_ok!(
                                iovs.slice(&memory, *iovs_len).map_err(mem_error_to_wasi)
                            );
                            let iovs_arr =
                                wasi_try_ok_ok!(iovs_arr.access().map_err(mem_error_to_wasi));
                            for iovs in iovs_arr.iter() {
                                let buf = wasi_try_ok_ok!(
                                    WasmPtr::<u8, M>::new(iovs.buf)
                                        .slice(&memory, iovs.buf_len)
                                        .map_err(mem_error_to_wasi)
                                );
                                let buf = wasi_try_ok_ok!(buf.access().map_err(mem_error_to_wasi));
                                let write_result = std::io::Write::write(pipe, buf.as_ref());
                                let local_written = match write_result {
                                    Ok(w) => w,
                                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                                        // Need to do this to avoid double borrow on ctx with iovs_arr
                                        raise_sigpipe = true;
                                        break;
                                    }
                                    Err(e) => return Ok(Err(map_io_err(e))),
                                };

                                written += local_written;
                                if local_written != buf.len() {
                                    break;
                                }
                            }

                            drop(iovs_arr);

                            if raise_sigpipe {
                                env.process.signal_process(Signal::Sigpipe);
                                wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);
                                return Ok(Err(Errno::Pipe));
                            }
                        }
                        FdWriteSource::Buffer(data) => {
                            match std::io::Write::write_all(pipe, data) {
                                Ok(()) => (),
                                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                                    env.process.signal_process(Signal::Sigpipe);
                                    wasi_try_ok_ok!(WasiEnv::process_signals_and_exit(ctx)?);
                                    return Ok(Err(Errno::Pipe));
                                }
                                Err(e) => return Ok(Err(map_io_err(e))),
                            };
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
                            let iovs_arr = wasi_try_ok_ok!(
                                iovs.slice(&memory, *iovs_len).map_err(mem_error_to_wasi)
                            );
                            let iovs_arr =
                                wasi_try_ok_ok!(iovs_arr.access().map_err(mem_error_to_wasi));
                            for iovs in iovs_arr.iter() {
                                let buf_len: usize = wasi_try_ok_ok!(
                                    iovs.buf_len.try_into().map_err(|_| Errno::Inval)
                                );
                                let will_be_written = buf_len;

                                let val_cnt = buf_len / std::mem::size_of::<u64>();
                                let val_cnt: M::Offset =
                                    wasi_try_ok_ok!(val_cnt.try_into().map_err(|_| Errno::Inval));

                                let vals = wasi_try_ok_ok!(
                                    WasmPtr::<u64, M>::new(iovs.buf)
                                        .slice(&memory, val_cnt as M::Offset)
                                        .map_err(mem_error_to_wasi)
                                );
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
                            let iovs_arr = wasi_try_ok_ok!(
                                iovs.slice(&memory, *iovs_len).map_err(mem_error_to_wasi)
                            );
                            let iovs_arr =
                                wasi_try_ok_ok!(iovs_arr.access().map_err(mem_error_to_wasi));
                            for iovs in iovs_arr.iter() {
                                let buf = wasi_try_ok_ok!(
                                    WasmPtr::<u8, M>::new(iovs.buf)
                                        .slice(&memory, iovs.buf_len)
                                        .map_err(mem_error_to_wasi)
                                );
                                let buf = wasi_try_ok_ok!(buf.access().map_err(mem_error_to_wasi));
                                let local_written = wasi_try_ok_ok!(
                                    std::io::Write::write(buffer, buf.as_ref()).map_err(map_io_err)
                                );
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
        if should_snapshot
            && can_snapshot
            && bytes_written > 0
            && let FdWriteSource::Iovs { iovs, iovs_len } = data
        {
            JournalEffector::save_fd_write(ctx, fd, offset, bytes_written, iovs, iovs_len)
                .map_err(|err| {
                    tracing::error!("failed to save terminal data - {}", err);
                    WasiError::Exit(ExitCode::from(Errno::Fault))
                })?;
        }

        env = ctx.data();
        memory = unsafe { env.memory_view(&ctx) };

        // reborrow and update the size
        if !is_stdio {
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
            let (mut memory, _, inodes) =
                unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
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
        }
        bytes_written
    };

    Ok(Ok(bytes_written))
}

#[cfg(all(test, feature = "sys"))]
mod tests {
    use wasmer::{Memory, MemoryType, Store, WasmPtr};
    use wasmer_wasix_types::wasi::Errno;

    use super::{FdWriteSource, MAX_STREAM_COALESCE};

    struct Guest {
        store: Store,
        memory: Memory,
    }

    impl Guest {
        fn new() -> Self {
            let mut store = Store::default();
            let memory = Memory::new(&mut store, MemoryType::new(1, Some(1), false)).unwrap();
            Self { store, memory }
        }

        /// Lays `payloads` out in guest memory, then writes an iovec array
        /// describing them. Each declared length is the payload length unless a
        /// test is lying about it.
        fn write_iovs(&mut self, payloads: &[&[u8]], claimed: Option<u32>) -> (u32, u32) {
            let view = self.memory.view(&self.store);
            let iovs_at = 8u32;
            let mut data_at = iovs_at + (payloads.len() as u32 * 8);

            for (i, payload) in payloads.iter().enumerate() {
                view.write(data_at as u64, payload).unwrap();
                let entry = iovs_at + (i as u32 * 8);
                view.write(entry as u64, &data_at.to_le_bytes()).unwrap();
                let len = claimed.unwrap_or(payload.len() as u32);
                view.write(entry as u64 + 4, &len.to_le_bytes()).unwrap();
                data_at += payload.len() as u32;
            }

            (iovs_at, payloads.len() as u32)
        }

        fn source(&self, iovs_at: u32, iovs_len: u32) -> FdWriteSource<'_, wasmer::Memory32> {
            FdWriteSource::Iovs {
                iovs: WasmPtr::new(iovs_at),
                iovs_len,
            }
        }
    }

    #[test]
    fn a_bounded_gather_walks_the_whole_writev() {
        let mut guest = Guest::new();
        let (iovs_at, iovs_len) = guest.write_iovs(&[b"abcd", b"efgh", b"ijkl"], None);
        let view = guest.memory.view(&guest.store);
        let source = guest.source(iovs_at, iovs_len);

        // Chunks smaller than the total let a caller walk it in steps, so the
        // bound never shortens the write the guest asked for.
        let mut seen = Vec::new();
        let mut skip = 0;
        loop {
            let chunk = source.coalesce_from(&view, skip, 5).unwrap();
            if chunk.is_empty() {
                break;
            }
            assert!(chunk.len() <= 5);
            skip += chunk.len();
            seen.extend_from_slice(&chunk);
        }
        assert_eq!(seen, b"abcdefghijkl");
    }

    #[test]
    fn a_bounded_gather_starts_mid_iovec() {
        let mut guest = Guest::new();
        let (iovs_at, iovs_len) = guest.write_iovs(&[b"abcd", b"efgh"], None);
        let view = guest.memory.view(&guest.store);
        let source = guest.source(iovs_at, iovs_len);

        assert_eq!(&*source.coalesce_from(&view, 2, 4).unwrap(), b"cdef");
        assert_eq!(&*source.coalesce_from(&view, 8, 4).unwrap(), b"");
    }

    #[test]
    fn a_lying_iovec_length_cannot_size_the_host_allocation() {
        // A guest owning one page can still claim 4 GiB. `coalesce` sizes its
        // buffer by that claim, which is why the stream path gathers in bounded
        // steps instead: a chunk never exceeds the limit, whatever is claimed.
        let mut guest = Guest::new();
        let (iovs_at, iovs_len) = guest.write_iovs(&[b"abcd"], Some(u32::MAX));
        let view = guest.memory.view(&guest.store);
        let source = guest.source(iovs_at, iovs_len);

        // The declared length outruns the page, so the read is caught; what
        // matters is that nothing reserved 4 GiB to find that out.
        assert_eq!(
            source
                .coalesce_from(&view, 0, MAX_STREAM_COALESCE)
                .unwrap_err(),
            Errno::Memviolation
        );

        // A truthful but oversized claim is gathered a chunk at a time.
        let (iovs_at, iovs_len) = guest.write_iovs(&[b"abcd"], None);
        let view = guest.memory.view(&guest.store);
        let source = guest.source(iovs_at, iovs_len);
        assert_eq!(source.coalesce_from(&view, 0, 2).unwrap().len(), 2);
    }

    #[test]
    fn a_datagram_is_refused_rather_than_split() {
        let mut guest = Guest::new();
        let (iovs_at, iovs_len) = guest.write_iovs(&[b"abcd", b"efgh"], None);
        let view = guest.memory.view(&guest.store);
        let source = guest.source(iovs_at, iovs_len);

        assert_eq!(source.coalesce(&view, 4).unwrap_err(), Errno::Msgsize);
        assert_eq!(&*source.coalesce(&view, 8).unwrap(), b"abcdefgh");
    }
}
