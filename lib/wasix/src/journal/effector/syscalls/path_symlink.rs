use super::*;

impl JournalEffector {
    pub fn save_path_symlink(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_path: String,
        fd: Fd,
        new_path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::CreateSymbolicLinkV1 {
                old_path: old_path.into(),
                fd,
                new_path: new_path.into(),
            },
        )
    }

    pub fn apply_path_symlink(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_path: &str,
        fd: Fd,
        new_path: &str,
    ) -> anyhow::Result<()> {
        crate::syscalls::path_symlink_internal(ctx, old_path, fd, new_path)
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to create symlink (old_path={}, fd={}, new_path={}) - {}",
                    old_path,
                    fd,
                    new_path,
                    err
                )
            })?;
        Ok(())
    }
}
