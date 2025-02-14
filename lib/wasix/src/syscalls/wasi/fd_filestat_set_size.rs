use super::*;
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
    wasi_try_ok!(fd_filestat_set_size_internal(&mut ctx, fd, st_size));
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
) -> Result<(), Errno> {
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = state.fs.get_fd(fd)?;
    let inode = fd_entry.inode;

    if !fd_entry.inner.rights.contains(Rights::FD_FILESTAT_SET_SIZE) {
        return Err(Errno::Access);
    }

    {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => {
                if let Some(handle) = handle {
                    let mut handle = handle.write().unwrap();
                    handle.set_len(st_size).map_err(fs_error_into_wasi_err)?;
                } else {
                    return Err(Errno::Badf);
                }
            }
            Kind::Buffer { buffer } => {
                buffer.resize(st_size as usize, 0);
            }
            Kind::Socket { .. }
            | Kind::PipeRx { .. }
            | Kind::PipeTx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::Symlink { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => return Err(Errno::Badf),
            Kind::Dir { .. } | Kind::Root { .. } => return Err(Errno::Isdir),
        }
    }
    inode.stat.write().unwrap().st_size = st_size;

    Ok(())
}
