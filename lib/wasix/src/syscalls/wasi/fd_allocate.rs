use super::*;
use crate::syscalls::*;

/// ### `fd_allocate`
/// Allocate extra space for a file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to allocate for
/// - `Filesize offset`
///     The offset from the start marking the beginning of the allocation
/// - `Filesize len`
///     The length from the offset marking the end of the allocation
#[instrument(level = "trace", skip_all, fields(%fd, %offset, %len), ret)]
pub fn fd_allocate(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    wasi_try_ok!(fd_allocate_internal(&mut ctx, fd, offset, len));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_allocate(&mut ctx, fd, offset, len).map_err(|err| {
            tracing::error!("failed to save file descriptor allocate event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn fd_allocate_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = state.fs.get_fd(fd)?;
    let inode = fd_entry.inode;

    if !fd_entry.inner.rights.contains(Rights::FD_ALLOCATE) {
        return Err(Errno::Access);
    }
    if len == 0 {
        return Err(Errno::Inval);
    }
    let new_size = offset.checked_add(len).ok_or(Errno::Inval)?;
    let mut current_size = inode.stat.read().unwrap().st_size;
    let mut resized = false;
    {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let mut handle = handle.write().unwrap();
                    let handle_size = handle.size();
                    if handle_size > current_size {
                        current_size = handle_size;
                    }
                    if new_size > current_size {
                        handle.set_len(new_size).map_err(fs_error_into_wasi_err)?;
                        resized = true;
                        current_size = new_size;
                    }
                } else {
                    return Err(Errno::Badf);
                }
            }
            Kind::Buffer { buffer } => {
                let buffer_size = buffer.len() as u64;
                if buffer_size > current_size {
                    current_size = buffer_size;
                }
                if new_size > current_size {
                    let new_size: usize = new_size.try_into().map_err(|_| Errno::Inval)?;
                    buffer.resize(new_size, 0);
                    resized = true;
                    current_size = new_size as u64;
                }
            }
            Kind::Socket { .. }
            | Kind::PipeRx { .. }
            | Kind::PipeTx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::Symlink { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. }
            | Kind::Dir { .. }
            | Kind::Root { .. } => return Err(Errno::Badf),
        }
    }
    {
        let mut stat = inode.stat.write().unwrap();
        if stat.st_size != current_size {
            stat.st_size = current_size;
        }
    }
    if resized {
        debug!(%new_size);
    }

    Ok(())
}
