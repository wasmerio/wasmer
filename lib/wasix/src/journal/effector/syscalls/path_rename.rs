use crate::{syscalls::__asyncify_light, VIRTUAL_ROOT_FD};

use super::*;

impl JournalEffector {
    pub fn save_path_rename(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_fd: Fd,
        old_path: String,
        new_fd: Fd,
        new_path: String,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::PathRenameV1 {
                old_fd,
                old_path: Cow::Owned(old_path),
                new_fd,
                new_path: Cow::Owned(new_path),
            },
        )
    }

    pub fn apply_path_rename(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_fd: Fd,
        old_path: &str,
        new_fd: Fd,
        new_path: &str,
    ) -> anyhow::Result<()> {
        // see `VIRTUAL_ROOT_FD` for details as to why this exists
        if old_fd == VIRTUAL_ROOT_FD && new_fd == VIRTUAL_ROOT_FD {
            let state = ctx.data().state.clone();
            let old_path = old_path.to_string();
            let new_path = new_path.to_string();
            __asyncify_light(ctx.data(), None, async move {
                state.fs_rename(old_path, new_path).await
            })??;
        } else {
            let ret =
                crate::syscalls::path_rename_internal(ctx, old_fd, old_path, new_fd, new_path)?;
            if ret != Errno::Success {
                bail!(
                    "journal restore error: failed to rename path (old_fd={}, old_path={}, new_fd={}, new_path={}) - {}",
                    old_fd,
                    old_path,
                    new_fd,
                    new_path,
                    ret
                );
            }
        }
        Ok(())
    }
}
