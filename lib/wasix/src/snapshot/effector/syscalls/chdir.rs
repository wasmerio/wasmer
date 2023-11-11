use super::*;

impl SnapshotEffector {
    pub fn save_chdir(ctx: &mut FunctionEnvMut<'_, WasiEnv>, path: String) -> anyhow::Result<()> {
        Self::save_event(ctx, SnapshotLog::ChangeDirectory { path: path.into() })
    }

    pub fn apply_chdir(ctx: &mut FunctionEnvMut<'_, WasiEnv>, path: &str) -> anyhow::Result<()> {
        crate::syscalls::chdir_internal(ctx, path).map_err(|err| {
            anyhow::format_err!(
                "snapshot restore error: failed to change directory (path={}) - {}",
                path,
                err
            )
        })?;
        Ok(())
    }
}
