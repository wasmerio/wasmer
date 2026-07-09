use std::path::PathBuf;

use anyhow::Context;

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

    let ret = wasi_try_ok!(__asyncify_light(
        env,
        None,
        path_rename_internal(env, old_fd, &source_str, new_fd, &target_str),
    )?);
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

pub async fn path_rename_internal(
    env: &WasiEnv,
    source_fd: WasiFd,
    source_path: &str,
    target_fd: WasiFd,
    target_path: &str,
) -> Result<Errno, Errno> {
    let state = env.state();
    let inodes = &state.inodes;

    let mut moved_ephemeral_symlink = false;

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

    // this is to be sure the source file is fetched from the filesystem if needed
    let source_inode = wasi_try_ok!(
        state
            .fs
            .get_inode_at_path(inodes, source_fd, source_path, true)
            .await
    );
    // Create the destination inode if the file exists.
    let _ = state
        .fs
        .get_inode_at_path(inodes, target_fd, target_path, true)
        .await;
    let (source_parent_inode, source_entry_name) = wasi_try_ok!(
        state
            .fs
            .get_parent_inode_at_path(inodes, source_fd, Path::new(source_path), true)
            .await
    );
    let (target_parent_inode, target_entry_name) = wasi_try_ok!(
        state
            .fs
            .get_parent_inode_at_path(inodes, target_fd, Path::new(target_path), true)
            .await
    );
    let source_guest_path = {
        let guard = source_parent_inode.read();
        match guard.deref() {
            Kind::Dir { path, .. } => crate::fs::PosixPath::from_path(path)
                .join(&crate::fs::PosixPath::new(&source_entry_name))
                .into_path_buf(),
            Kind::Root { .. } => return Ok(Errno::Notcapable),
            Kind::Socket { .. }
            | Kind::PipeTx { .. }
            | Kind::PipeRx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => return Ok(Errno::Inval),
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                debug!("fatal internal logic error: parent of inode is not a directory");
                return Ok(Errno::Inval);
            }
        }
    };
    let mut need_create = true;
    let target_guest_path = {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if entries.contains_key(&target_entry_name) {
                    need_create = false;
                }
                crate::fs::PosixPath::from_path(path)
                    .join(&crate::fs::PosixPath::new(&target_entry_name))
                    .into_path_buf()
            }
            Kind::Root { .. } => return Ok(Errno::Notcapable),
            Kind::Socket { .. }
            | Kind::PipeTx { .. }
            | Kind::PipeRx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => return Ok(Errno::Inval),
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                debug!("fatal internal logic error: parent of inode is not a directory");
                return Ok(Errno::Inval);
            }
        }
    };

    if source_parent_inode.ino() == target_parent_inode.ino()
        && source_entry_name == target_entry_name
    {
        return Ok(Errno::Success);
    }

    let source_is_dir = {
        let guard = source_inode.read();
        matches!(guard.deref(), Kind::Dir { .. })
    };
    if source_is_dir
        && crate::fs::PosixPath::from_path(&target_guest_path)
            .strip_prefix(&crate::fs::PosixPath::from_path(&source_guest_path))
            .is_some()
    {
        return Ok(Errno::Inval);
    }

    let source_entry = {
        let mut guard = source_parent_inode.write();
        match guard.deref_mut() {
            Kind::Dir { entries, .. } => {
                wasi_try_ok!(entries.remove(&source_entry_name).ok_or(Errno::Noent))
            }
            Kind::Root { .. } => return Ok(Errno::Notcapable),
            Kind::Socket { .. }
            | Kind::PipeRx { .. }
            | Kind::PipeTx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => {
                return Ok(Errno::Inval);
            }
            Kind::Symlink { .. } | Kind::File { .. } | Kind::Buffer { .. } => {
                debug!("fatal internal logic error: parent of inode is not a directory");
                return Ok(Errno::Inval);
            }
        }
    };

    enum RenameSource {
        File(PathBuf),
        Dir(PathBuf),
        Symlink(PathBuf),
        Other,
    }

    let rename_source = {
        let guard = source_entry.read();
        match guard.deref() {
            Kind::File { path, .. } => RenameSource::File(path.clone()),
            Kind::Dir { path, .. } => RenameSource::Dir(path.clone()),
            Kind::Symlink { relative_path, .. } => RenameSource::Symlink(relative_path.clone()),
            Kind::Buffer { .. }
            | Kind::Socket { .. }
            | Kind::PipeTx { .. }
            | Kind::PipeRx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::Epoll { .. }
            | Kind::EventNotifications { .. } => RenameSource::Other,
            Kind::Root { .. } => unreachable!("The root can not be moved"),
        }
    };

    match rename_source {
        RenameSource::File(path) => {
            let result = state.fs_rename(path, &target_guest_path).await;
            // if the above operation failed we have to revert the previous change and then fail
            if let Err(e) = result {
                let mut guard = source_parent_inode.write();
                if let Kind::Dir { entries, .. } = guard.deref_mut() {
                    entries.insert(source_entry_name, source_entry.clone());
                    return Ok(e);
                }
            } else {
                let mut guard = source_entry.write();
                if let Kind::File { path, .. } = guard.deref_mut() {
                    *path = target_guest_path.clone();
                } else {
                    unreachable!()
                }
            }
        }
        RenameSource::Dir(source_dir_path) => {
            let res = state
                .fs_rename(source_dir_path.clone(), &target_guest_path)
                .await;
            if let Err(e) = res {
                return Ok(e);
            }
            rename_inode_tree(&source_entry, &source_dir_path, &target_guest_path);
        }
        RenameSource::Symlink(relative_path) => {
            let is_ephemeral = state
                .fs
                .ephemeral_symlink_at(source_guest_path.as_path())
                .is_some();
            let from = source_guest_path.clone();
            let to = target_guest_path.clone();
            let res = state.fs_rename(from, to).await;
            match (res, is_ephemeral) {
                (Ok(()), _) | (Err(Errno::Noent), true) => {}
                (Err(e), _) => {
                    let mut guard = source_parent_inode.write();
                    if let Kind::Dir { entries, .. } = guard.deref_mut() {
                        entries.insert(source_entry_name, source_entry.clone());
                        return Ok(e);
                    }
                }
            }

            let new_path_to_symlink = state
                .fs
                .rebase_symlink_location(target_guest_path.as_path());
            {
                let mut guard = source_entry.write();
                let Kind::Symlink {
                    path_to_symlink, ..
                } = guard.deref_mut()
                else {
                    unreachable!()
                };
                *path_to_symlink = new_path_to_symlink.clone();
            }
            if is_ephemeral {
                state.fs.move_ephemeral_symlink(
                    source_guest_path.as_path(),
                    target_guest_path.as_path(),
                    new_path_to_symlink,
                    relative_path,
                );
                moved_ephemeral_symlink = true;
            }
        }
        RenameSource::Other => {}
    }

    let source_size = source_entry.stat.read().unwrap().st_size;

    if need_create {
        let mut guard = target_parent_inode.write();
        if let Kind::Dir { entries, .. } = guard.deref_mut() {
            let result = entries.insert(target_entry_name.clone(), source_entry);
            assert!(
                result.is_none(),
                "fatal error: race condition on filesystem detected or internal logic error"
            );
        }
    }

    // The target entry is created, one way or the other
    let target_inode = state
        .fs
        .get_inode_at_path(inodes, target_fd, target_path, true)
        .await
        .expect("Expected target inode to exist, and it's too late to safely fail");
    *target_inode.name.write().unwrap() = target_entry_name.into();
    target_inode.stat.write().unwrap().st_size = source_size;

    // If the rename replaced an existing destination entry, clear any stale
    // ephemeral symlink mapping for that path.
    if !moved_ephemeral_symlink {
        state
            .fs
            .unregister_ephemeral_symlink(target_guest_path.as_path());
    }

    Ok(Errno::Success)
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
    let relative_path = path
        .strip_prefix(&source_dir_path)
        .with_context(|| format!("Expected path {path:?} to be a subpath of {source_dir_path:?}"))
        .expect("Fatal filesystem error");
    crate::fs::PosixPath::from_path(target_dir_path)
        .join(&relative_path)
        .into_path_buf()
}
