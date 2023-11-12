use super::*;
use crate::syscalls::*;

/// ### `tty_set()`
/// Updates the properties of the rect
#[instrument(level = "debug", skip_all, ret)]
pub fn tty_set<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    tty_state: WasmPtr<Tty, M>,
) -> Errno {
    let env = ctx.data();
    let bridge = if let Some(t) = env.runtime.tty() {
        t
    } else {
        return Errno::Notsup;
    };

    let memory = unsafe { env.memory_view(&ctx) };
    let state = wasi_try_mem!(tty_state.read(&memory));
    let echo = state.echo;
    let line_buffered = state.line_buffered;
    let line_feeds = true;
    debug!(
        %echo,
        %line_buffered,
        %line_feeds
    );

    let state = crate::runtime::TtyState {
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

    bridge.tty_set(state);

    Errno::Success
}
