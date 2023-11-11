use super::*;

impl SnapshotEffector {
    pub fn save_path_unlink(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            SnapshotLog::UnlinkFile {
                fd,
                path: Cow::Owned(path),
            },
        )
    }

    pub fn apply_path_unlink(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: &str,
    ) -> anyhow::Result<()> {
        let ret = crate::syscalls::path_unlink_file_internal(ctx, fd, path)?;
        if ret != Errno::Success {
            bail!(
                "snapshot restore error: failed to remove file (fd={}, path={}) - {}",
                fd,
                path,
                ret
            );
        }
        Ok(())
    }
}
