use super::*;
use std::sync::atomic::Ordering;
use crate::syscalls::*;

/// ### `fd_filestat_set_size()`
/// Change the size of an open file, zeroing out any new bytes
/// Inputs:
/// - `Fd fd`
///     File descriptor to adjust
/// - `Filesize st_size`
///     New size that `fd` will be set to
#[instrument(level = "trace", skip_all, fields(%fd, %st_size), ret)]
pub fn fd_filestat_set_size(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_size: Filesize,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    wasi_try_ok!(fd_filestat_set_size_internal(&mut ctx, fd, st_size)?);
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_set_size(&mut ctx, fd, st_size).map_err(|err| {
            tracing::error!("failed to save file set size event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn fd_filestat_set_size_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    st_size: Filesize,
) -> WasiResult<()> {
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = wasi_try_ok_ok!(state.fs.get_fd(fd));
    let inode = fd_entry.inode;
    let old_size = inode.stat.read().unwrap().st_size;

    {
        let guard = inode.read();
        match guard.deref() {
            Kind::File { .. } | Kind::Buffer { .. } => {}
            Kind::Socket { .. }
            | Kind::PipeRx { .. }
            | Kind::PipeTx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::Symlink { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. }
            | Kind::Dir { .. }
            | Kind::Root { .. } => return Ok(Err(Errno::Badf)),
        }
    }

    if !fd_entry.inner.rights.contains(Rights::FD_FILESTAT_SET_SIZE) {
        return Ok(Err(Errno::Access));
    }

    {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if fd_entry.open_flags & crate::fs::Fd::WRITE == 0 {
                    // Match Linux ftruncate behavior on read-only fds.
                    return Ok(Err(Errno::Inval));
                }
                let handle = match handle {
                    Some(handle) => handle.clone(),
                    None => return Ok(Err(Errno::Badf)),
                };
                let original_offset = fd_entry.inner.offset.load(Ordering::Acquire);

                drop(guard);
                wasi_try_ok_ok!(__asyncify(ctx, None, async move {
                    let mut handle = handle.write().unwrap();
                    handle.set_len(st_size).map_err(fs_error_into_wasi_err)?;

                    if st_size > old_size {
                        handle
                            .seek(std::io::SeekFrom::Start(old_size))
                            .await
                            .map_err(map_io_err)?;

                        let mut remaining = st_size - old_size;
                        let zeros = [0u8; 8192];
                        while remaining > 0 {
                            let chunk = std::cmp::min(remaining, zeros.len() as u64) as usize;
                            handle
                                .write_all(&zeros[..chunk])
                                .await
                                .map_err(map_io_err)?;
                            remaining -= chunk as u64;
                        }

                        handle
                            .seek(std::io::SeekFrom::Start(original_offset))
                            .await
                            .map_err(map_io_err)?;
                    }

                    Ok(())
                })?);
            }
            Kind::Buffer { buffer } => {
                if fd_entry.open_flags & crate::fs::Fd::WRITE == 0 {
                    // Match Linux ftruncate behavior on read-only fds.
                    return Ok(Err(Errno::Inval));
                }
                buffer.resize(st_size as usize, 0);
            }
            Kind::Socket { .. }
            | Kind::PipeRx { .. }
            | Kind::PipeTx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::Symlink { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. }
            | Kind::Dir { .. }
            | Kind::Root { .. } => return Ok(Err(Errno::Badf)),
        }
    }
    inode.stat.write().unwrap().st_size = st_size;

    Ok(Ok(()))
}
