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

    let env = ctx.data();
    let bridge = if let Some(t) = env.runtime.tty() {
        t
    } else {
        return Errno::Notsup;
    };

    let state = bridge.tty_get();
    let tty_out = Tty {
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

    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem!(tty_state.write(&memory, tty_out));
    {
        let line_feeds_ptr = wasi_try!(tty_line_feeds_ptr(tty_state));
        let line_feeds_raw = if state.line_feeds { 1u8 } else { 0u8 };
        wasi_try_mem!(line_feeds_ptr.write(&memory, line_feeds_raw));
    }

    Errno::Success
}
