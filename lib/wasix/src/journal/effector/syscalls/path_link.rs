use super::*;

impl JournalEffector {
    pub fn save_path_link(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_fd: Fd,
        old_flags: LookupFlags,
        old_path: String,
        new_fd: Fd,
        new_path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::CreateHardLinkV1 {
                old_fd,
                old_flags,
                old_path: old_path.into(),
                new_fd,
                new_path: new_path.into(),
            },
        )
    }

    pub fn apply_path_link(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_fd: Fd,
        old_flags: LookupFlags,
        old_path: &str,
        new_fd: Fd,
        new_path: &str,
    ) -> anyhow::Result<()> {
        crate::syscalls::path_link_internal(ctx, old_fd, old_flags, old_path, new_fd, new_path)
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to create hard link (old_fd={}, old_flags={}, old_path={}, new_fd={}, new_path={}) - {}",
                    old_fd,
                    old_flags,
                    old_path,
                    new_fd,
                    new_path,
                    err
                )
            })?;
        Ok(())
    }
}
