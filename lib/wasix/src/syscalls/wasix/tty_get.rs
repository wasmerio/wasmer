use super::*;
use crate::syscalls::*;

/// ### `tty_get()`
/// Retrieves the current state of the TTY
#[instrument(level = "trace", skip_all, ret)]
pub fn tty_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    tty_state: WasmPtr<Tty, M>,
) -> Errno {
    let env = ctx.data();
    let bridge = if let Some(t) = env.tty() {
        t
    } else {
        return Errno::Notsup;
    };

    // Redirection replaces the guest descriptors without changing the host TTY.
    let is_stdio = |fd| env.state.fs.get_fd(fd).is_ok_and(|entry| entry.is_stdio);
    let state = bridge.tty_get();
    let state = Tty {
        cols: state.cols,
        rows: state.rows,
        width: state.width,
        height: state.height,
        stdin_tty: state.stdin_tty && is_stdio(__WASI_STDIN_FILENO),
        stdout_tty: state.stdout_tty && is_stdio(__WASI_STDOUT_FILENO),
        stderr_tty: state.stderr_tty && is_stdio(__WASI_STDERR_FILENO),
        echo: state.echo,
        line_buffered: state.line_buffered,
    };

    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem!(tty_state.write(&memory, state));

    Errno::Success
}
