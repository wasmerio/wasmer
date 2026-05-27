use super::*;
use crate::fs::{FlushPoller, InodeKindWriteGuard, MAX_FD, lock_inodes_for_renumber};
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

fn dup_fd_entry(from: &Fd) -> Fd {
    Fd {
        inner: FdInner {
            offset: from.inner.offset.clone(),
            rights: from.inner.rights_inheriting,
            fd_flags: {
                let mut f = from.inner.fd_flags;
                f.set(Fdflagsext::CLOEXEC, false);
                f
            },
            ..from.inner
        },
        inode: from.inode.clone(),
        ..*from
    }
}

pub(crate) fn fd_renumber_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    from: WasiFd,
    to: WasiFd,
) -> Result<Errno, WasiError> {
    if to > MAX_FD {
        return Ok(Errno::Badf);
    }

    if from == to {
        let fd_map = ctx.data().state.fs.fd_map.read().unwrap();
        return Ok(if fd_map.get(from).is_some() {
            Errno::Success
        } else {
            Errno::Badf
        });
    }

    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let (from_inode, target_inode, same_inode) = {
        let fd_map = state.fs.fd_map.read().unwrap();
        let from_entry = match fd_map.get(from) {
            Some(entry) => entry,
            None => return Ok(Errno::Badf),
        };
        if let Some(target_fd) = fd_map.get(to)
            && !target_fd.is_stdio
            && target_fd.inode.is_preopened
        {
            warn!("Refusing fd_renumber({from}, {to}) because FD {to} is pre-opened");
            return Ok(Errno::Notsup);
        }
        let target_inode = fd_map.get(to).map(|entry| entry.inode.clone());
        let same_inode = target_inode
            .as_ref()
            .is_some_and(|inode| from_entry.inode.same_inode_as(inode));
        (from_entry.inode.clone(), target_inode, same_inode)
    };

    let old_fd;
    if same_inode {
        let mut kind = InodeKindWriteGuard::new(&from_inode);
        let mut fd_map = state.fs.fd_map.write().unwrap();

        let fd_entry = match fd_map.get(from) {
            Some(entry) if entry.inode.same_inode_as(&from_inode) => entry,
            _ => return Ok(Errno::Badf),
        };
        if let Some(target_fd) = fd_map.get(to)
            && !target_fd.is_stdio
            && target_fd.inode.is_preopened
        {
            return Ok(Errno::Notsup);
        }

        let new_fd_entry = dup_fd_entry(fd_entry);
        old_fd = match fd_map.replace(to, new_fd_entry, &mut kind) {
            Ok(old) => old,
            Err(()) => return Ok(Errno::Badf),
        };
    } else {
        let (from_kind, target_kind) = lock_inodes_for_renumber(&from_inode, target_inode.as_ref());
        let mut fd_map = state.fs.fd_map.write().unwrap();

        let fd_entry = match fd_map.get(from) {
            Some(entry) if entry.inode.same_inode_as(&from_inode) => entry,
            _ => return Ok(Errno::Badf),
        };
        if let Some(target_fd) = fd_map.get(to)
            && !target_fd.is_stdio
            && target_fd.inode.is_preopened
        {
            return Ok(Errno::Notsup);
        }

        let new_fd_entry = dup_fd_entry(fd_entry);

        old_fd = if let Some(expected_target) = target_inode.as_ref() {
            match fd_map.get(to) {
                Some(entry) if entry.inode.same_inode_as(expected_target) => {
                    fd_map.remove(to, target_kind.expect("target guard"))
                }
                Some(_) => return Ok(Errno::Badf),
                None => None,
            }
        } else if fd_map.get(to).is_some() {
            return Ok(Errno::Badf);
        } else {
            None
        };

        if !fd_map.insert(true, to, new_fd_entry, from_kind) {
            return Ok(Errno::Badf);
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
