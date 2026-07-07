use std::path::Path;

use virtual_fs::FileSystem;

use crate::VIRTUAL_ROOT_FD;

use super::*;

impl JournalEffector {
    pub fn save_path_unlink(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::UnlinkFileV1 {
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
        // see `VIRTUAL_ROOT_FD` for details as to why this exists
        if fd == VIRTUAL_ROOT_FD {
            crate::syscalls::__asyncify_light(ctx.data(), None, async {
                ctx.data()
                    .state
                    .fs
                    .root_fs
                    .remove_file(Path::new(path))
                    .await
                    .map_err(crate::fs::fs_error_into_wasi_err)
            })??;
        } else {
            let ret = crate::syscalls::__asyncify_light(
                ctx.data(),
                None,
                crate::syscalls::path_unlink_file_internal(ctx.data(), fd, path),
            )??;
            if ret != Errno::Success {
                bail!(
                    "journal restore error: failed to remove file (fd={fd}, path={path}) - {ret}"
                );
            }
        }
        Ok(())
    }
}
