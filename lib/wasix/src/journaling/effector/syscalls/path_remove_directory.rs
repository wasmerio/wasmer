use super::*;

impl JournalEffector {
    pub fn save_path_remove_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::RemoveDirectory {
                fd,
                path: Cow::Owned(path),
            },
        )
    }

    pub fn apply_path_remove_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: &str,
    ) -> anyhow::Result<()> {
        if let Err(err) = crate::syscalls::path_remove_directory_internal(ctx, fd, path) {
            bail!(
                "journal restore error: failed to remove directory - {}",
                err
            );
        }
        Ok(())
    }
}
