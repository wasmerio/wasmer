use super::*;
use crate::syscalls::*;

/// ### `path_unlink_file()`
/// Unlink a file, deleting if the number of hardlinks is 1
/// Inputs:
/// - `Fd fd`
///     The base file descriptor from which the path is understood
/// - `const char *path`
///     Array of UTF-8 bytes representing the path
/// - `u32 path_len`
///     The number of bytes in the `path` array
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_unlink_file<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let base_dir = wasi_try_ok!(state.fs.get_fd(fd));
    if !base_dir.inner.rights.contains(Rights::PATH_UNLINK_FILE) {
        return Ok(Errno::Access);
    }
    let path_str = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    let ret = wasi_try_ok!(__asyncify_light(
        env,
        None,
        path_unlink_file_internal(env, fd, &path_str)
    )?);
    let env = ctx.data();

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        if env.enable_journal {
            wasi_try_ok!(
                JournalEffector::save_path_unlink(&mut ctx, fd, path_str).map_err(|err| {
                    tracing::error!("failed to save unlink event - {}", err);
                    Errno::Fault
                })
            )
        }
    }

    Ok(ret)
}

pub(crate) async fn path_unlink_file_internal(
    env: &WasiEnv,
    fd: WasiFd,
    path: &str,
) -> Result<Errno, Errno> {
    let state = env.state();
    let inodes = &state.inodes;

    let inode = wasi_try_ok!(state.fs.get_inode_at_path(inodes, fd, path, false).await);
    let (parent_inode, child_name) = wasi_try_ok!(
        state
            .fs
            .get_parent_inode_at_path(inodes, fd, std::path::Path::new(path), false)
            .await
    );
    let host_adjusted_path = {
        let guard = parent_inode.read();
        match guard.deref() {
            Kind::Dir { path, .. } => path.join(&child_name),
            Kind::Root { .. } => return Ok(Errno::Access),
            _ => unreachable!(
                "Internal logic error in wasi::path_unlink_file, parent is not a directory"
            ),
        }
    };

    let removed_inode = {
        let mut guard = parent_inode.write();
        match guard.deref_mut() {
            Kind::Dir { entries, .. } => entries.remove(&child_name),
            Kind::Root { .. } => return Ok(Errno::Access),
            _ => unreachable!(
                "Internal logic error in wasi::path_unlink_file, parent is not a directory"
            ),
        }
    };
    let Some(removed_inode) = removed_inode else {
        let inode_is_symlink = matches!(inode.read().deref(), Kind::Symlink { .. });
        if !inode_is_symlink {
            tracing::warn!(
                "wasi::path_unlink_file: path resolution returned inode {:?} for {:?}, but parent directory had no matching entry",
                inode.ino(),
                child_name
            );
            return Ok(Errno::Noent);
        }
        return Ok(state
            .fs
            .remove_symlink_file(host_adjusted_path.as_path())
            .await);
    };
    {
        // TODO: make this a debug assert in the future
        assert!(inode.ino() == removed_inode.ino());
        debug_assert!(inode.stat.read().unwrap().st_nlink > 0);
    }

    let st_nlink = {
        let mut guard = removed_inode.stat.write().unwrap();
        guard.st_nlink -= 1;
        guard.st_nlink
    };
    if st_nlink == 0 {
        enum RemoveTarget {
            OpenFile(crate::fs::VirtualFileLock),
            ClosedFile(std::path::PathBuf),
            Symlink,
        }

        let target = {
            let guard = removed_inode.read();
            match guard.deref() {
                Kind::File { handle, path, .. } => match handle {
                    Some(h) => RemoveTarget::OpenFile(h.clone()),
                    None => RemoveTarget::ClosedFile(path.clone()),
                },
                Kind::Dir { .. } | Kind::Root { .. } => return Ok(Errno::Isdir),
                Kind::Symlink { .. } => RemoveTarget::Symlink,
                _ => unimplemented!("wasi::path_unlink_file for Buffer"),
            }
        };

        match target {
            RemoveTarget::OpenFile(h) => {
                let mut h = h.lock().await;
                wasi_try_ok!(h.unlink().await.map_err(fs_error_into_wasi_err));
            }
            RemoveTarget::ClosedFile(path) => {
                wasi_try_ok!(state.fs_remove_file(path).await);
            }
            RemoveTarget::Symlink => {
                let errno = state
                    .fs
                    .remove_symlink_file(host_adjusted_path.as_path())
                    .await;
                if errno != Errno::Success {
                    return Ok(errno);
                }
            }
        }
    }

    Ok(Errno::Success)
}
