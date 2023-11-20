use super::*;
use crate::syscalls::*;

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `Fd from`
///     File descriptor to copy
/// - `Fd to`
///     Location to copy file descriptor to
#[instrument(level = "debug", skip_all, fields(%from, %to), ret)]
pub fn fd_renumber(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    from: WasiFd,
    to: WasiFd,
) -> Result<Errno, WasiError> {
    let ret = fd_renumber_internal(&mut ctx, from, to);
    let env = ctx.data();

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        if env.enable_journal {
            JournalEffector::save_fd_renumber(&mut ctx, from, to).map_err(|err| {
                tracing::error!("failed to save file descriptor renumber event - {}", err);
                WasiError::Exit(ExitCode::Errno(Errno::Fault))
            })?;
        }
    }
    Ok(ret)
}

pub(crate) fn fd_renumber_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    from: WasiFd,
    to: WasiFd,
) -> Errno {
    if from == to {
        return Errno::Success;
    }
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let mut fd_map = state.fs.fd_map.write().unwrap();
    let fd_entry = wasi_try!(fd_map.get_mut(&from).ok_or(Errno::Badf));

    let new_fd_entry = Fd {
        // TODO: verify this is correct
        offset: fd_entry.offset.clone(),
        rights: fd_entry.rights_inheriting,
        inode: fd_entry.inode.clone(),
        ..*fd_entry
    };
    fd_map.insert(to, new_fd_entry);
    state.fs.make_max_fd(to + 1);

    Errno::Success
}
