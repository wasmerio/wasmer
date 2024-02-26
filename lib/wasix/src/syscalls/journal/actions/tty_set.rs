use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_tty_set(
        &mut self,
        tty: Tty,
        line_feeds: bool,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!("Replay journal - TtySet");
        let state = crate::WasiTtyState {
            cols: tty.cols,
            rows: tty.rows,
            width: tty.width,
            height: tty.height,
            stdin_tty: tty.stdin_tty,
            stdout_tty: tty.stdout_tty,
            stderr_tty: tty.stderr_tty,
            echo: tty.echo,
            line_buffered: tty.line_buffered,
            line_feeds,
        };

        JournalEffector::apply_tty_set(&mut self.ctx, state).map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
