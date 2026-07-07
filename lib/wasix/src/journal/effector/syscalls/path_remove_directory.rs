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
            crate::syscalls::__asyncify_light(
                ctx.data(),
                None,
                async {
                    ctx.data()
                        .state
                        .fs
                        .root_fs
                        .remove_dir(Path::new(path))
                        .await
                        .map_err(crate::fs::fs_error_into_wasi_err)
                },
            )??;
        } else {
            let base_dir = ctx.data().state.fs.get_fd(fd).map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: invalid directory descriptor (fd={fd}) - {err}"
                )
            })?;
            if let Err(err) = crate::syscalls::__asyncify_light(
                ctx.data(),
                None,
                crate::syscalls::path_remove_directory_internal(ctx.data(), fd, base_dir, path),
            )? {
                bail!("journal restore error: failed to remove directory - {err}");
            }
        }
        Ok(())
    }
}
