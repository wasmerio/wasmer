use super::*;
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
    if from == to {
        return Ok(Errno::Success);
    }
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    // Flush the target FD before acquiring the write lock, since flushing
    // may perform async I/O and we don't want to hold the lock during that.
    if let Ok(fd) = state.fs.get_fd(to) {
        if !fd.is_stdio && fd.inode.is_preopened {
            warn!(
                "FD ({to}) is a pre-open and should not be closed, \
                but will be closed in response to an fd_renumber operation. \
                This will likely break stuff."
            );
        }
        match __asyncify_light(env, None, state.fs.flush(to))? {
            Ok(_) | Err(Errno::Isdir) | Err(Errno::Io) | Err(Errno::Access) => {}
            Err(e) => {
                return Ok(e);
            }
        }
    }

    // Hold a single write lock for both the close and insert to prevent
    // another thread from allocating into the target slot between the two
    // operations.
    let mut fd_map = state.fs.fd_map.write().unwrap();

    // Remove the target FD under the same lock (replaces the separate
    // close_fd call which would acquire its own lock).
    let _ = fd_map.remove(to);

    let fd_entry = wasi_try_ok!(fd_map.get(from).ok_or(Errno::Badf));

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

    if !fd_map.insert(true, to, new_fd_entry) {
        panic!("Internal error: expected FD {to} to be free after closing in fd_renumber");
    }

    Ok(Errno::Success)
}
