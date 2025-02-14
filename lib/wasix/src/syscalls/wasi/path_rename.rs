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
    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

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
    wasi_try_ok!(state
        .fs
        .get_inode_at_path(inodes, source_fd, source_path, true));
    // Create the destination inode if the file exists.
    let _ = state
        .fs
        .get_inode_at_path(inodes, target_fd, target_path, true);
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
    let mut need_create = true;
    let host_adjusted_target_path = {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if entries.contains_key(&target_entry_name) {
                    need_create = false;
                }
                path.join(&target_entry_name)
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

    {
        let mut guard = source_entry.write();
        match guard.deref_mut() {
            Kind::File { ref path, .. } => {
                let result = {
                    let path_clone = path.clone();
                    drop(guard);
                    let state = state;
                    let host_adjusted_target_path = host_adjusted_target_path.clone();
                    __asyncify_light(env, None, async move {
                        state
                            .fs_rename(path_clone, &host_adjusted_target_path)
                            .await
                    })?
                };
                // if the above operation failed we have to revert the previous change and then fail
                if let Err(e) = result {
                    let mut guard = source_parent_inode.write();
                    if let Kind::Dir { entries, .. } = guard.deref_mut() {
                        entries.insert(source_entry_name, source_entry);
                        return Ok(e);
                    }
                } else {
                    let mut guard = source_entry.write();
                    if let Kind::File { ref mut path, .. } = guard.deref_mut() {
                        *path = host_adjusted_target_path;
                    } else {
                        unreachable!()
                    }
                }
            }
            Kind::Dir { ref path, .. } => {
                let cloned_path = path.clone();
                let res = {
                    let state = state;
                    let host_adjusted_target_path = host_adjusted_target_path.clone();
                    __asyncify_light(env, None, async move {
                        state
                            .fs_rename(cloned_path, &host_adjusted_target_path)
                            .await
                    })?
                };
                if let Err(e) = res {
                    return Ok(e);
                }
                {
                    let source_dir_path = path.clone();
                    drop(guard);
                    rename_inode_tree(&source_entry, &source_dir_path, &host_adjusted_target_path);
                }
            }
            Kind::Buffer { .. }
            | Kind::Symlink { .. }
            | Kind::Socket { .. }
            | Kind::PipeTx { .. }
            | Kind::PipeRx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::Epoll { .. }
            | Kind::EventNotifications { .. } => {}
            Kind::Root { .. } => unreachable!("The root can not be moved"),
        }
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
        .expect("Expected target inode to exist, and it's too late to safely fail");
    *target_inode.name.write().unwrap() = target_entry_name.into();
    target_inode.stat.write().unwrap().st_size = source_size;

    Ok(Errno::Success)
}

fn rename_inode_tree(inode: &InodeGuard, source_dir_path: &Path, target_dir_path: &Path) {
    let children;

    let mut guard = inode.write();
    match guard.deref_mut() {
        Kind::File { ref mut path, .. } => {
            *path = adjust_path(path, source_dir_path, target_dir_path);
            return;
        }
        Kind::Dir {
            ref mut path,
            entries,
            ..
        } => {
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
    let relative_path = path
        .strip_prefix(source_dir_path)
        .with_context(|| format!("Expected path {path:?} to be a subpath of {source_dir_path:?}"))
        .expect("Fatal filesystem error");
    target_dir_path.join(relative_path)
}
