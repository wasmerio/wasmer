use crate::WasiTtyState;

use super::*;

impl SnapshotEffector {
    pub fn save_tty_set(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        state: WasiTtyState,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            SnapshotLog::TtySet {
                tty: wasmer_wasix_types::wasi::Tty {
                    cols: state.cols,
                    rows: state.rows,
                    width: state.width,
                    height: state.height,
                    stdin_tty: state.stdin_tty,
                    stdout_tty: state.stdout_tty,
                    stderr_tty: state.stderr_tty,
                    echo: state.echo,
                    line_buffered: state.line_buffered,
                },
                line_feeds: state.line_feeds,
            },
        )
    }

    pub fn apply_tty_set(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        state: WasiTtyState,
    ) -> anyhow::Result<()> {
        crate::syscalls::tty_set_internal(ctx, state).map_err(|err| {
            anyhow::format_err!("snapshot restore error: failed to set tty - {}", err)
        })?;
        Ok(())
    }
}
