use super::*;
use crate::syscalls::*;

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
#[instrument(level = "trace", skip_all, fields(%fd, %offset, ?whence), ret)]
pub fn fd_seek<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: FileDelta,
    whence: Whence,
    newoffset: WasmPtr<Filesize, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(WasiEnv::process_signals_and_exit(&mut ctx)?);

    let new_offset = wasi_try_ok!(fd_seek_internal(&mut ctx, fd, offset, whence)?);
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_seek(&mut ctx, fd, offset, whence).map_err(|err| {
            tracing::error!("failed to save file descriptor seek event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    // reborrow
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let new_offset_ref = newoffset.deref(&memory);
    let fd_entry = wasi_try_ok!(env.state.fs.get_fd(fd));
    wasi_try_mem_ok!(new_offset_ref.write(new_offset));

    trace!(
        %new_offset,
    );

    Ok(Errno::Success)
}

pub(crate) fn fd_seek_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: FileDelta,
    whence: Whence,
) -> Result<Result<Filesize, Errno>, WasiError> {
    let env = ctx.data();
    let state = env.state.clone();
    let (memory, _) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_SEEK) {
        return Ok(Err(Errno::Access));
    }

    // TODO: handle case if fd is a dir?
    let new_offset = match whence {
        Whence::Cur => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));

            #[allow(clippy::comparison_chain)]
            if offset > 0 {
                let offset = offset as u64;
                fd_entry.offset.fetch_add(offset, Ordering::AcqRel) + offset
            } else if offset < 0 {
                let offset = offset.unsigned_abs();
                // FIXME: need to handle underflow!
                fd_entry.offset.fetch_sub(offset, Ordering::AcqRel) - offset
            } else {
                fd_entry.offset.load(Ordering::Acquire)
            }
        }
        Whence::End => {
            use std::io::SeekFrom;
            let mut guard = fd_entry.inode.write();
            let deref_mut = guard.deref_mut();
            match deref_mut {
                Kind::File { ref mut handle, .. } => {
                    // TODO: remove allow once inodes are refactored (see comments on [`WasiState`])
                    #[allow(clippy::await_holding_lock)]
                    if let Some(handle) = handle {
                        let handle = handle.clone();
                        drop(guard);

                        wasi_try_ok_ok!(__asyncify(ctx, None, async move {
                            let mut handle = handle.write().unwrap();
                            let end = handle
                                .seek(SeekFrom::End(offset))
                                .await
                                .map_err(map_io_err)?;

                            // TODO: handle case if fd_entry.offset uses 64 bits of a u64
                            drop(handle);
                            let mut fd_map = state.fs.fd_map.write().unwrap();
                            let fd_entry = fd_map.get_mut(&fd).ok_or(Errno::Badf)?;
                            fd_entry.offset.store(end, Ordering::Release);
                            Ok(())
                        })?);
                    } else {
                        return Ok(Err(Errno::Inval));
                    }
                }
                Kind::Symlink { .. } => {
                    unimplemented!("wasi::fd_seek not implemented for symlinks")
                }
                Kind::Dir { .. }
                | Kind::Root { .. }
                | Kind::Socket { .. }
                | Kind::Pipe { .. }
                | Kind::EventNotifications { .. }
                | Kind::Epoll { .. } => {
                    // TODO: check this
                    return Ok(Err(Errno::Inval));
                }
                Kind::Buffer { .. } => {
                    // seeking buffers probably makes sense
                    // FIXME: implement this
                    return Ok(Err(Errno::Inval));
                }
            }
            fd_entry.offset.load(Ordering::Acquire)
        }
        Whence::Set => {
            let mut fd_map = state.fs.fd_map.write().unwrap();
            let fd_entry = wasi_try_ok_ok!(fd_map.get_mut(&fd).ok_or(Errno::Badf));
            fd_entry.offset.store(offset as u64, Ordering::Release);
            offset as u64
        }
        _ => return Ok(Err(Errno::Inval)),
    };

    Ok(Ok(new_offset))
}
