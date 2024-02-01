use super::*;
use crate::{syscalls::*, WasiTtyState};

/// ### `tty_set()`
/// Updates the properties of the rect
#[instrument(level = "debug", skip_all, ret)]
pub fn tty_set<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    tty_state: WasmPtr<Tty, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();

    let memory = unsafe { env.memory_view(&ctx) };
    let state = wasi_try_mem_ok!(tty_state.read(&memory));
    let echo = state.echo;
    let line_buffered = state.line_buffered;
    let line_feeds = true;
    debug!(
        %echo,
        %line_buffered,
        %line_feeds
    );

    let state = crate::os::tty::WasiTtyState {
        cols: state.cols,
        rows: state.rows,
        width: state.width,
        height: state.height,
        stdin_tty: state.stdin_tty,
        stdout_tty: state.stdout_tty,
        stderr_tty: state.stderr_tty,
        echo,
        line_buffered,
        line_feeds,
    };

    wasi_try_ok!({
        #[allow(clippy::redundant_clone)]
        tty_set_internal(&mut ctx, state.clone())
    });
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_tty_set(&mut ctx, state).map_err(|err| {
            tracing::error!("failed to save path symbolic link event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub fn tty_set_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    state: WasiTtyState,
) -> Result<(), Errno> {
    let env = ctx.data();
    let bridge = if let Some(t) = env.runtime.tty() {
        t
    } else {
        return Err(Errno::Notsup);
    };
    bridge.tty_set(state);

    Ok(())
}
