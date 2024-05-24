use std::path::Path;

use virtual_fs::FileSystem;

use crate::VIRTUAL_ROOT_FD;

use super::*;

impl JournalEffector {
    pub fn save_path_create_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::CreateDirectoryV1 {
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
        // see `VIRTUAL_ROOT_FD` for details as to why this exists
        if fd == VIRTUAL_ROOT_FD {
            ctx.data().state.fs.root_fs.create_dir(Path::new(path))?;
        } else {
            crate::syscalls::path_create_directory_internal(ctx, fd, path).map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to create directory path (fd={}, path={}) - {}",
                    fd,
                    path,
                    err
                )
            })?;
        }
        Ok(())
    }
}
