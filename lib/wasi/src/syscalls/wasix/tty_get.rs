use super::*;
use crate::syscalls::*;

/// ### `tty_get()`
/// Retrieves the current state of the TTY
pub fn tty_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    tty_state: WasmPtr<Tty, M>,
) -> Errno {
    debug!("wasi[{}:{}]::tty_get", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();

    let state = env.runtime.tty_get();
    let state = Tty {
        cols: state.cols,
        rows: state.rows,
        width: state.width,
        height: state.height,
        stdin_tty: state.stdin_tty,
        stdout_tty: state.stdout_tty,
        stderr_tty: state.stderr_tty,
        echo: state.echo,
        line_buffered: state.line_buffered,
    };

    let memory = env.memory_view(&ctx);
    wasi_try_mem!(tty_state.write(&memory, state));

    Errno::Success
}