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

    if let Ok(fd) = state.fs.get_fd(to) {
        if !fd.is_stdio && fd.inode.is_preopened {
            // There isn't a good hack we can do here; the code that made this call
            // expects its new FD to be the number it asked for. This will, however,
            // break wasix-libc when it attempts to use the FD to make any fs-related
            // syscalls. The best we can do is warn people so they can change the code.
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
        wasi_try_ok!(state.fs.close_fd(to));
    }

    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try_ok!(fd_map.get(from).ok_or(Errno::Badf));

    let new_fd_entry = Fd {
        // TODO: verify this is correct
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

    // Exclusive insert because we expect `to` to be empty after closing it above
    if !fd_map.insert(true, to, new_fd_entry) {
        panic!("Internal error: expected FD {to} to be free after closing in fd_renumber");
    }

    Ok(Errno::Success)
}
