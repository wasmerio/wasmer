use std::path::Path;

use virtual_fs::FileSystem;

use crate::VIRTUAL_ROOT_FD;

use super::*;

impl JournalEffector {
    pub fn save_path_remove_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::RemoveDirectoryV1 {
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
        // see `VIRTUAL_ROOT_FD` for details as to why this exists
        if fd == VIRTUAL_ROOT_FD {
            ctx.data().state.fs.root_fs.remove_dir(Path::new(path))?;
        } else if let Err(err) = crate::syscalls::path_remove_directory_internal(ctx, fd, path) {
            bail!(
                "journal restore error: failed to remove directory - {}",
                err
            );
        }
        Ok(())
    }
}
