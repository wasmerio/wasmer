use std::path::PathBuf;

use super::*;
use crate::syscalls::*;

/// ### `path_rename()`
/// Rename a file or directory
/// Inputs:
/// - `Fd old_fd`
///     The base directory for `old_path`
/// - `const char* old_path`
///     Pointer to UTF8 bytes, the file to be renamed
/// - `u32 old_path_len`
///     The number of bytes to read from `old_path`
/// - `Fd new_fd`
///     The base directory for `new_path`
/// - `const char* new_path`
///     Pointer to UTF8 bytes, the new file name
/// - `u32 new_path_len`
///     The number of bytes to read from `new_path`
#[instrument(level = "trace", skip_all, fields(%old_fd, %new_fd, old_path = field::Empty, new_path = field::Empty), ret)]
pub fn path_rename<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    old_fd: WasiFd,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    new_fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let source_str = unsafe { get_input_str_ok!(&memory, old_path, old_path_len) };
    Span::current().record("old_path", source_str.as_str());
    let target_str = unsafe { get_input_str_ok!(&memory, new_path, new_path_len) };
    Span::current().record("new_path", target_str.as_str());

    let ret = path_rename_internal(&mut ctx, old_fd, &source_str, new_fd, &target_str)?;
    let env = ctx.data();

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        if env.enable_journal {
            JournalEffector::save_path_rename(&mut ctx, old_fd, source_str, new_fd, target_str)
                .map_err(|err| {
                    tracing::error!("failed to save path rename event - {}", err);
                    WasiError::Exit(ExitCode::from(Errno::Fault))
                })?;
        }
    }
    Ok(ret)
}

pub fn path_rename_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    source_fd: WasiFd,
    source_path: &str,
    target_fd: WasiFd,
    target_path: &str,
) -> Result<Errno, WasiError> {
    rename_at(ctx.data(), source_fd, source_path, target_fd, target_path)
}

/// Renames `source_path` (relative to `source_fd`) to `target_path` (relative
/// to `target_fd`), atomically replacing an existing target like POSIX
/// `rename(2)`.
///
/// The backing filesystem performs the rename first. Only after it succeeded
/// are the cached directory entries updated, so a failed or cancelled backing
/// rename leaves the cache untouched.
fn rename_at(
    env: &WasiEnv,
    source_fd: WasiFd,
    source_path: &str,
    target_fd: WasiFd,
    target_path: &str,
) -> Result<Errno, WasiError> {
    let (state, inodes) = env.get_wasi_state_and_inodes();

    {
        let source_fd = wasi_try_ok!(state.fs.get_fd(source_fd));
        if !source_fd.inner.rights.contains(Rights::PATH_RENAME_SOURCE) {
            return Ok(Errno::Access);
        }
        let target_fd = wasi_try_ok!(state.fs.get_fd(target_fd));
        if !target_fd.inner.rights.contains(Rights::PATH_RENAME_TARGET) {
            return Ok(Errno::Access);
        }
    }

    let _namespace_guard = state.fs.lock_namespace();

    // This also loads the source into the cache if needed. The last component
    // is not followed: renaming a symlink moves the link itself.
    let source_inode = wasi_try_ok!(state.fs.get_inode_at_path(
        inodes,
        source_fd,
        source_path,
        false
    ));
    let (source_parent_inode, source_entry_name) = wasi_try_ok!(state.fs.get_parent_inode_at_path(
        inodes,
        source_fd,
        Path::new(source_path),
        true
    ));
    let (target_parent_inode, target_entry_name) = wasi_try_ok!(state.fs.get_parent_inode_at_path(
        inodes,
        target_fd,
        Path::new(target_path),
        true
    ));
    let (source_guest_path, cached_source) =
        wasi_try_ok!(directory_entry(&source_parent_inode, &source_entry_name));
    let (target_guest_path, cached_target) =
        wasi_try_ok!(directory_entry(&target_parent_inode, &target_entry_name));

    // If both paths name the same file (including two hard links to it),
    // rename does nothing.
    if (source_parent_inode.ino() == target_parent_inode.ino()
        && source_entry_name == target_entry_name)
        || cached_target
            .as_ref()
            .is_some_and(|target| target.is_same_inode(&source_inode))
    {
        return Ok(Errno::Success);
    }

    let source_is_dir = matches!(source_inode.read().deref(), Kind::Dir { .. });
    if source_is_dir
        && crate::fs::PosixPath::from_path(&target_guest_path)
            .strip_prefix(&crate::fs::PosixPath::from_path(&source_guest_path))
            .is_some()
    {
        return Ok(Errno::Inval);
    }

    // Rename in the backing filesystem. Inodes without a backing file
    // (pipes, sockets, ...) only move within the cache.
    let (backing_source, is_ephemeral_symlink) = match source_inode.read().deref() {
        Kind::File { path, .. } | Kind::Dir { path, .. } => (Some(path.clone()), false),
        Kind::Symlink { .. } => (
            Some(source_guest_path.clone()),
            state
                .fs
                .ephemeral_symlink_at(source_guest_path.as_path())
                .is_some(),
        ),
        Kind::Root { .. } => return Ok(Errno::Busy),
        Kind::Buffer { .. }
        | Kind::Socket { .. }
        | Kind::PipeTx { .. }
        | Kind::PipeRx { .. }
        | Kind::DuplexPipe { .. }
        | Kind::Epoll { .. }
        | Kind::EventNotifications { .. } => (None, false),
    };
    if let Some(from) = backing_source {
        let to = target_guest_path.clone();
        let result = __asyncify_light(env, None, async move { state.fs_rename(from, to).await })?;
        match result {
            Ok(()) => {}
            // An ephemeral symlink has no backing file.
            Err(Errno::Noent) if is_ephemeral_symlink => {}
            Err(err) => return Ok(err),
        }
    }

    // Point the moved inode at its new location.
    let moved_dir_path = {
        let mut guard = source_inode.write();
        match guard.deref_mut() {
            Kind::File { path, .. } => {
                *path = target_guest_path.clone();
                None
            }
            Kind::Dir { path, .. } => Some(path.clone()),
            Kind::Symlink {
                path_to_symlink,
                relative_path,
                ..
            } => {
                let new_path_to_symlink = state
                    .fs
                    .rebase_symlink_location(target_guest_path.as_path());
                *path_to_symlink = new_path_to_symlink.clone();
                if is_ephemeral_symlink {
                    state.fs.move_ephemeral_symlink(
                        source_guest_path.as_path(),
                        target_guest_path.as_path(),
                        new_path_to_symlink,
                        relative_path.clone(),
                    );
                }
                None
            }
            _ => None,
        }
    };
    if let Some(source_dir_path) = moved_dir_path {
        rename_inode_tree(&source_inode, &source_dir_path, &target_guest_path);
    }
    *source_inode.name.write().unwrap() = target_entry_name.clone().into();

    // Move the cached entry. The backing rename replaced whatever the target
    // name referred to, and any entry still cached for either name describes
    // the state before the rename, possibly one a concurrent lookup inserted
    // in the meantime. Inodes that were not cached (such as backing symlinks)
    // stay uncached.
    let source_was_cached = cached_source.is_some_and(|source| source.is_same_inode(&source_inode));
    if let Kind::Dir { entries, .. } = source_parent_inode.write().deref_mut() {
        entries.remove(&source_entry_name);
    }
    if let Kind::Dir { entries, .. } = target_parent_inode.write().deref_mut() {
        if source_was_cached {
            entries.insert(target_entry_name, source_inode);
        } else {
            entries.remove(&target_entry_name);
        }
    }

    // If the rename replaced an existing destination entry, clear any stale
    // ephemeral symlink mapping for that path.
    if !is_ephemeral_symlink {
        state
            .fs
            .unregister_ephemeral_symlink(target_guest_path.as_path());
    }

    Ok(Errno::Success)
}

/// Returns the guest path of the entry `name` in the directory `parent`, and
/// the inode cached for that entry, if any.
fn directory_entry(
    parent: &InodeGuard,
    name: &str,
) -> Result<(PathBuf, Option<InodeGuard>), Errno> {
    match parent.read().deref() {
        Kind::Dir { entries, path, .. } => Ok((
            crate::fs::PosixPath::from_path(path)
                .join(&crate::fs::PosixPath::new(name))
                .into_path_buf(),
            entries.get(name).cloned(),
        )),
        Kind::Root { .. } => Err(Errno::Notcapable),
        Kind::Socket { .. }
        | Kind::PipeTx { .. }
        | Kind::PipeRx { .. }
        | Kind::DuplexPipe { .. }
        | Kind::EventNotifications { .. }
        | Kind::Epoll { .. } => Err(Errno::Inval),
        Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
            debug!("fatal internal logic error: parent of inode is not a directory");
            Err(Errno::Inval)
        }
    }
}

fn rename_inode_tree(inode: &InodeGuard, source_dir_path: &Path, target_dir_path: &Path) {
    let children;

    let mut guard = inode.write();
    match guard.deref_mut() {
        Kind::File { path, .. } => {
            *path = adjust_path(path, source_dir_path, target_dir_path);
            return;
        }
        Kind::Dir { path, entries, .. } => {
            *path = adjust_path(path, source_dir_path, target_dir_path);
            children = entries.values().cloned().collect::<Vec<_>>();
        }
        _ => return,
    }
    drop(guard);

    for child in children {
        rename_inode_tree(&child, source_dir_path, target_dir_path);
    }
}

fn adjust_path(path: &Path, source_dir_path: &Path, target_dir_path: &Path) -> PathBuf {
    let path = crate::fs::PosixPath::from_path(path);
    let source_dir_path = crate::fs::PosixPath::from_path(source_dir_path);
    let Some(relative_path) = path.strip_prefix(&source_dir_path) else {
        // A directory tree can contain an inode also referenced by a hard link
        // outside the moved tree. Keep that alias's cached path unchanged.
        return PathBuf::from(path.as_str());
    };
    crate::fs::PosixPath::from_path(target_dir_path)
        .join(&relative_path)
        .into_path_buf()
}

#[cfg(all(test, feature = "sys-thread", not(target_arch = "wasm32")))]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures::future::BoxFuture;
    use virtual_fs::{FileSystem, FsError, Metadata, OpenOptions, ReadDir};

    use super::*;
    use crate::fs::VIRTUAL_ROOT_FD;

    type Hook = Box<dyn FnOnce() + Send>;

    /// A memory filesystem whose next rename can fail, or run a hook after it
    /// succeeded, i.e. between the backing rename and the cache update.
    #[derive(Default)]
    struct HookedFs {
        inner: virtual_fs::mem_fs::FileSystem,
        after_rename: Mutex<Option<Hook>>,
        fail_rename: Mutex<Option<FsError>>,
    }

    impl std::fmt::Debug for HookedFs {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("HookedFs").finish_non_exhaustive()
        }
    }

    impl FileSystem for HookedFs {
        fn readlink(&self, path: &Path) -> virtual_fs::Result<PathBuf> {
            self.inner.readlink(path)
        }
        fn read_dir(&self, path: &Path) -> virtual_fs::Result<ReadDir> {
            self.inner.read_dir(path)
        }
        fn create_dir(&self, path: &Path) -> virtual_fs::Result<()> {
            self.inner.create_dir(path)
        }
        fn remove_dir(&self, path: &Path) -> virtual_fs::Result<()> {
            self.inner.remove_dir(path)
        }
        fn rename<'a>(
            &'a self,
            from: &'a Path,
            to: &'a Path,
        ) -> BoxFuture<'a, virtual_fs::Result<()>> {
            Box::pin(async move {
                if let Some(err) = self.fail_rename.lock().unwrap().take() {
                    return Err(err);
                }
                self.inner.rename(from, to).await?;
                if let Some(hook) = self.after_rename.lock().unwrap().take() {
                    hook();
                }
                Ok(())
            })
        }
        fn metadata(&self, path: &Path) -> virtual_fs::Result<Metadata> {
            self.inner.metadata(path)
        }
        fn symlink_metadata(&self, path: &Path) -> virtual_fs::Result<Metadata> {
            self.inner.symlink_metadata(path)
        }
        fn remove_file(&self, path: &Path) -> virtual_fs::Result<()> {
            self.inner.remove_file(path)
        }
        fn new_open_options(&self) -> OpenOptions<'_> {
            self.inner.new_open_options()
        }
    }

    struct Fixture {
        fs: Arc<HookedFs>,
        env: WasiEnv,
        _runtime: tokio::runtime::Runtime,
    }

    impl Fixture {
        /// A WASI environment on a [`HookedFs`] containing `dirs` and `files`.
        fn new(dirs: &[&str], files: &[&str]) -> Self {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();
            let _guard = runtime.enter();

            let fs = Arc::new(HookedFs::default());
            for dir in dirs {
                fs.inner.create_dir(Path::new(dir)).unwrap();
            }
            for file in files {
                fs.inner
                    .new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(file)
                    .unwrap();
            }

            let env = WasiEnv::builder("path-rename")
                .engine(wasmer::Engine::default())
                .fs(fs.clone() as Arc<dyn FileSystem + Send + Sync>)
                .preopen_dir("/")
                .unwrap()
                .build()
                .unwrap();

            Self {
                fs,
                env,
                _runtime: runtime,
            }
        }

        fn inode(&self, path: &str) -> InodeGuard {
            let (state, inodes) = self.env.get_wasi_state_and_inodes();
            state
                .fs
                .get_inode_at_path(inodes, VIRTUAL_ROOT_FD, path, false)
                .unwrap()
        }

        fn rename(&self, from: &str, to: &str) -> Errno {
            rename_at(&self.env, VIRTUAL_ROOT_FD, from, VIRTUAL_ROOT_FD, to).unwrap()
        }

        /// The inodes cached in the directory at `path`, by name.
        fn cached_entries(&self, path: &str) -> HashMap<String, InodeGuard> {
            match self.inode(path).read().deref() {
                Kind::Dir { entries, .. } => entries.clone(),
                _ => panic!("{path} is not a directory"),
            }
        }
    }

    /// Another thread looking up the target while the backing rename is in
    /// flight caches an inode for it. The rename must replace that entry.
    /// This used to trip an assertion while holding the directory's write
    /// lock, which also poisoned the lock for every later access.
    #[test]
    fn rename_replaces_a_target_cached_during_the_backing_rename() {
        let fixture = Fixture::new(&["/dir"], &["/dir/tmp"]);
        let source = fixture.inode("/dir/tmp");

        let state = fixture.env.state.clone();
        *fixture.fs.after_rename.lock().unwrap() = Some(Box::new(move || {
            state
                .fs
                .get_inode_at_path(&state.inodes, VIRTUAL_ROOT_FD, "/dir/target", false)
                .expect("the renamed file exists in the backing filesystem");
        }));

        assert_eq!(fixture.rename("/dir/tmp", "/dir/target"), Errno::Success);

        let entries = fixture.cached_entries("/dir");
        assert!(!entries.contains_key("tmp"));
        assert!(entries["target"].is_same_inode(&source));
        assert!(matches!(
            source.read().deref(),
            Kind::File { path, .. } if path == Path::new("/dir/target")
        ));
    }

    /// Replacing a cached target must drop its inode from the cache. It used
    /// to stay cached, so later opens reused the replaced file's handle.
    #[test]
    fn rename_replaces_a_cached_target() {
        let fixture = Fixture::new(&["/dir"], &["/dir/tmp", "/dir/target"]);
        let source = fixture.inode("/dir/tmp");
        let replaced = fixture.inode("/dir/target");

        assert_eq!(fixture.rename("/dir/tmp", "/dir/target"), Errno::Success);

        let entries = fixture.cached_entries("/dir");
        assert!(!entries.contains_key("tmp"));
        assert!(entries["target"].is_same_inode(&source));
        assert!(!entries["target"].is_same_inode(&replaced));
        assert!(fixture.inode("/dir/target").is_same_inode(&source));
    }

    /// POSIX: renaming a file onto another hard link to it does nothing.
    #[test]
    fn rename_onto_a_hard_link_to_the_same_file_does_nothing() {
        let fixture = Fixture::new(&["/dir"], &["/dir/a"]);
        let file = fixture.inode("/dir/a");
        if let Kind::Dir { entries, .. } = fixture.inode("/dir").write().deref_mut() {
            // How path_link links a file on a filesystem without hard links.
            entries.insert("b".to_string(), file.clone());
        }

        assert_eq!(fixture.rename("/dir/a", "/dir/b"), Errno::Success);

        let entries = fixture.cached_entries("/dir");
        assert!(entries["a"].is_same_inode(&file));
        assert!(entries["b"].is_same_inode(&file));
        assert!(fixture.fs.inner.metadata(Path::new("/dir/a")).is_ok());
    }

    /// A failed backing rename must leave the cache as it was. For
    /// directories the source entry used to be dropped from the cache.
    #[test]
    fn failed_backing_rename_leaves_the_cache_untouched() {
        let fixture = Fixture::new(&["/dir", "/dir/source"], &["/dir/target"]);
        let source = fixture.inode("/dir/source");
        let target = fixture.inode("/dir/target");
        *fixture.fs.fail_rename.lock().unwrap() = Some(FsError::PermissionDenied);

        assert_eq!(fixture.rename("/dir/source", "/dir/target"), Errno::Perm);

        let entries = fixture.cached_entries("/dir");
        assert!(entries["source"].is_same_inode(&source));
        assert!(entries["target"].is_same_inode(&target));
    }
}
