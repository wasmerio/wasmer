use super::*;

impl JournalEffector {
    pub fn save_path_create_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::CreateDirectory {
                fd,
                path: path.into(),
            },
        )
    }

    pub fn apply_path_create_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: &str,
    ) -> anyhow::Result<()> {
        crate::syscalls::path_create_directory_internal(ctx, fd, path).map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to create directory path (fd={}, path={}) - {}",
                fd,
                path,
                err
            )
        })?;
        Ok(())
    }
}
