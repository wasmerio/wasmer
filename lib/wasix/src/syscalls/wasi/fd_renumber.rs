use super::*;
use crate::fs::FlushPoller;
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
    let env = ctx.data();
    let (_, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let flush_target = match state.fs.dup2_at(from, to) {
        Err(errno) => return Ok(errno),
        Ok(flush_target) => flush_target,
    };

    // Best-effort flush of the replaced entry; result depends only on map updates.
    if let Some(file) = flush_target {
        let _ = __asyncify_light(env, None, FlushPoller { file })?;
    }

    Ok(Errno::Success)
}
