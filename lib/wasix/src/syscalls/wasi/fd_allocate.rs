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

    wasi_try_ok!(__asyncify_light(
        ctx.data(),
        None,
        fd_allocate_internal(ctx.data(), fd, offset, len)
    )?);
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

pub(crate) async fn fd_allocate_internal(
    env: &WasiEnv,
    fd: WasiFd,
    offset: Filesize,
    len: Filesize,
) -> Result<(), Errno> {
    let mut state = env.state();
    let fd_entry = state.fs.get_fd(fd)?;
    let inode = fd_entry.inode;

    if !fd_entry.inner.rights.contains(Rights::FD_ALLOCATE) {
        return Err(Errno::Access);
    }
    let new_size = offset.checked_add(len).ok_or(Errno::Inval)?;
    let mut current_size = 0;
    let handle = {
        let mut guard = inode.write();
        match guard.deref_mut() {
            Kind::File { handle, .. } => Some(handle.clone().ok_or(Errno::Badf)?),
            Kind::Buffer { buffer } => {
                current_size = buffer.len() as u64;
                if new_size > current_size {
                    buffer.resize(new_size as usize, 0);
                    current_size = new_size;
                }
                None
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
    };
    if let Some(handle) = handle {
        let mut handle = handle.lock().await;
        current_size = handle.size().await;
        if new_size > current_size {
            handle
                .set_len(new_size)
                .await
                .map_err(fs_error_into_wasi_err)?;
            current_size = new_size;
        }
    }
    inode.stat.write().unwrap().st_size = current_size;
    debug!(%new_size);

    Ok(())
}
