use super::*;

impl JournalEffector {
    pub fn save_fd_duplicate(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        original_fd: Fd,
        copied_fd: Fd,
        cloexec: bool,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::DuplicateFileDescriptorV2 {
                original_fd,
                copied_fd,
                cloexec,
            },
        )
    }

    pub fn apply_fd_duplicate(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        original_fd: Fd,
        copied_fd: Fd,
        cloexec: bool,
    ) -> anyhow::Result<()> {
        let ret_fd = crate::syscalls::fd_dup_internal(ctx, original_fd, 0, cloexec)
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to duplicate file descriptor (original={}, copied={}) - {}",
                    original_fd,
                    copied_fd,
                    err
                )
            })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd, copied_fd);
        if !matches!(ret, Ok(Errno::Success)) {
            bail!(
                "journal restore error: failed renumber file descriptor after duplicate (from={}, to={}) - {}",
                ret_fd,
                copied_fd,
                ret.unwrap_or(Errno::Unknown)
            );
        }

        let ret = crate::syscalls::fd_fdflags_set_internal(
            ctx,
            copied_fd,
            if cloexec {
                Fdflagsext::CLOEXEC
            } else {
                Fdflagsext::empty()
            },
        );
        if !matches!(ret, Ok(Errno::Success)) {
            bail!(
                "journal restore error: failed renumber file descriptor after duplicate (from={}, to={}) - {}",
                ret_fd,
                copied_fd,
                ret.unwrap_or(Errno::Unknown)
            );
        }

        Ok(())
    }
}
