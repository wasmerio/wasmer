use wasmer_wasix_types::wasi::Signal;

use super::*;

impl JournalEffector {
    pub fn save_thread_exit(
        env: &WasiEnv,
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    ) -> anyhow::Result<()> {
        env.active_journal()?
            .write(JournalEntry::CloseThreadV1 {
                id: id.raw(),
                exit_code,
            })
            .map_err(map_snapshot_err)?;
        Ok(())
    }

    pub fn apply_thread_exit(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        tid: WasiThreadId,
        exit_code: Option<ExitCode>,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        if let Some(thread) = env.process.get_thread(&tid) {
            if let Some(code) = exit_code {
                thread.set_status_finished(Ok(code));
            }
            thread.signal(Signal::Sigkill);
        }
        Ok(())
    }
}
