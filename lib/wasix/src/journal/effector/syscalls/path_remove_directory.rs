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
        let base_dir = ctx.data().state.fs.get_fd(fd).map_err(|err| {
            anyhow::format_err!(
                "journal restore error: invalid directory descriptor (fd={fd}) - {err}"
            )
        })?;
        if let Err(err) = crate::syscalls::path_remove_directory_internal(ctx, fd, base_dir, path) {
            bail!("journal restore error: failed to remove directory - {err}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use virtual_fs::{FileSystem, FsError, TmpFileSystem};
    use wasmer::{Engine, Store};

    use crate::{VIRTUAL_ROOT_FD, WasiEnvBuilder, WasiFunctionEnv};

    use super::*;

    #[tokio::test]
    async fn journal_remove_directory_resolves_virtual_root_entries() {
        let backing = Arc::new(TmpFileSystem::new());
        backing.create_dir(Path::new("/journal-dir")).unwrap();
        let env = WasiEnvBuilder::new("test")
            .engine(Engine::default())
            .fs(backing.clone() as Arc<dyn FileSystem + Send + Sync>)
            .build()
            .unwrap();
        let mut store = Store::default();
        let function_env = WasiFunctionEnv::new(&mut store, env);
        let mut ctx = function_env.env.into_mut(&mut store);

        ctx.data().state.fs.register_ephemeral_symlink(
            "/journal-dir/link".into(),
            "journal-dir/link".into(),
            "target".into(),
        );
        assert!(
            JournalEffector::apply_path_remove_directory(&mut ctx, VIRTUAL_ROOT_FD, "journal-dir")
                .is_err()
        );
        assert!(
            backing
                .metadata(Path::new("/journal-dir"))
                .unwrap()
                .is_dir()
        );
        ctx.data()
            .state
            .fs
            .unregister_ephemeral_symlink(Path::new("/journal-dir/link"));

        JournalEffector::apply_path_remove_directory(&mut ctx, VIRTUAL_ROOT_FD, "journal-dir")
            .unwrap();

        assert_eq!(
            backing.symlink_metadata(Path::new("/journal-dir")),
            Err(FsError::EntryNotFound)
        );
    }
}
