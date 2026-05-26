use super::*;
use crate::fs::{FlushPoller, InodeKindWriteGuard, MAX_FD};
use crate::syscalls::*;

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `Fd from`
///     File descriptor to copy
/// - `Fd to`
///     Location to copy file descriptor to
#[instrument(level = "trace", skip_all, fields(%from, %to), ret)]
pub fn fd_renumber(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    from: WasiFd,
    to: WasiFd,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let ret = fd_renumber_internal(&mut ctx, from, to)?;
    let env = ctx.data();

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        if env.enable_journal {
            JournalEffector::save_fd_renumber(&mut ctx, from, to).map_err(|err| {
                tracing::error!("failed to save file descriptor renumber event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            })?;
        }
    }

    Ok(ret)
}

pub(crate) fn fd_renumber_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    from: WasiFd,
    to: WasiFd,
) -> Result<Errno, WasiError> {
    if to > MAX_FD {
        return Ok(Errno::Badf);
    }
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let from_inode = wasi_try_ok!(state.fs.get_fd(from)).inode.clone();
    if from == to {
        return Ok(Errno::Success);
    }

    if let Some(target_fd) = state.fs.get_fd(to).ok()
        && !target_fd.is_stdio
        && target_fd.inode.is_preopened
    {
        warn!("Refusing fd_renumber({from}, {to}) because FD {to} is pre-opened");
        return Ok(Errno::Notsup);
    }

    let mut from_kind = InodeKindWriteGuard::new(&from_inode);
    let target_inode = state
        .fs
        .get_fd(to)
        .ok()
        .map(|target_fd| target_fd.inode.clone());
    let mut target_kind = target_inode
        .as_ref()
        .map(|inode| InodeKindWriteGuard::new(inode));

    let old_fd;
    {
        let mut fd_map = state.fs.fd_map.write().unwrap();

        let fd_entry = wasi_try_ok!(fd_map.get(from).ok_or(Errno::Badf));

        if let Some(target_fd) = fd_map.get(to)
            && !target_fd.is_stdio
            && target_fd.inode.is_preopened
        {
            return Ok(Errno::Notsup);
        }

        let new_fd_entry = Fd {
            inner: FdInner {
                offset: fd_entry.inner.offset.clone(),
                rights: fd_entry.inner.rights_inheriting,
                fd_flags: {
                    let mut f = fd_entry.inner.fd_flags;
                    f.set(Fdflagsext::CLOEXEC, false);
                    f
                },
                ..fd_entry.inner
            },
            inode: fd_entry.inode.clone(),
            ..*fd_entry
        };

        old_fd = if let Some(target_kind) = target_kind.take() {
            fd_map.remove(to, target_kind)
        } else {
            None
        };

        if !fd_map.insert(true, to, new_fd_entry, from_kind) {
            panic!("Internal error: expected FD {to} to be free after closing in fd_renumber");
        }
    }

    // Flush and drop the old FD outside the lock. The flush is best-effort:
    // failures are intentionally ignored so fd_renumber result depends only on
    // descriptor map updates and validation.
    let flush_target = old_fd.as_ref().and_then(|fd_entry| {
        let guard = fd_entry.inode.read();
        match guard.deref() {
            Kind::File {
                handle: Some(file), ..
            } => Some(file.clone()),
            _ => None,
        }
    });
    drop(old_fd);

    if let Some(file) = flush_target {
        let _ = __asyncify_light(env, None, FlushPoller { file })?;
    }

    Ok(Errno::Success)
}
