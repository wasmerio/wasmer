use super::*;
use crate::syscalls::*;
use virtual_fs::FsError;

/// ### `path_symlink()`
/// Create a symlink
/// Inputs:
/// - `const char *old_path`
///     Array of UTF-8 bytes representing the source path
/// - `u32 old_path_len`
///     The number of bytes to read from `old_path`
/// - `Fd fd`
///     The base directory from which the paths are understood
/// - `const char *new_path`
///     Array of UTF-8 bytes representing the target path
/// - `u32 new_path_len`
///     The number of bytes to read from `new_path`
#[instrument(level = "trace", skip_all, fields(%fd, old_path = field::Empty, new_path = field::Empty), ret)]
pub fn path_symlink<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    old_path: WasmPtr<u8, M>,
    old_path_len: M::Offset,
    fd: WasiFd,
    new_path: WasmPtr<u8, M>,
    new_path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let old_path_str = unsafe { get_input_str_ok!(&memory, old_path, old_path_len) };
    Span::current().record("old_path", old_path_str.as_str());
    let new_path_str = unsafe { get_input_str_ok!(&memory, new_path, new_path_len) };
    Span::current().record("new_path", new_path_str.as_str());

    wasi_try_ok!(path_symlink_internal(
        &mut ctx,
        &old_path_str,
        fd,
        &new_path_str
    ));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_path_symlink(&mut ctx, old_path_str, fd, new_path_str).map_err(
            |err| {
                tracing::error!("failed to save path symbolic link event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            },
        )?;
    }

    Ok(Errno::Success)
}

pub fn path_symlink_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    old_path: &str,
    fd: WasiFd,
    new_path: &str,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let base_fd = state.fs.get_fd(fd)?;
    if !base_fd.inner.rights.contains(Rights::PATH_SYMLINK) {
        return Err(Errno::Access);
    }

    if std::path::Path::new(old_path).is_absolute() {
        return Err(Errno::Notsup);
    }

    // get the depth of the parent + 1 (UNDER INVESTIGATION HMMMMMMMM THINK FISH ^ THINK FISH)
    let old_path_path = std::path::Path::new(old_path);
    let (source_inode, _) = state
        .fs
        .get_parent_inode_at_path(inodes, fd, old_path_path, true)?;
    let depth = state.fs.path_depth_from_fd(fd, source_inode);

    // depth == -1 means folder is not relative. See issue #3233.
    let depth = match depth {
        Ok(depth) => depth as i32 - 1,
        Err(_) => -1,
    };

    let new_path_path = std::path::Path::new(new_path);
    let (target_parent_inode, entry_name) =
        state
            .fs
            .get_parent_inode_at_path(inodes, fd, new_path_path, true)?;
    let trailing_slash = new_path.as_bytes().last() == Some(&b'/');

    if trailing_slash {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if let Some(child_inode) = entries.get(&entry_name) {
                    let child_guard = child_inode.read();
                    return if matches!(child_guard.deref(), Kind::Dir { .. }) {
                        Err(Errno::Exist)
                    } else {
                        Err(Errno::Notdir)
                    };
                }
                let mut child_path = path.clone();
                child_path.push(&entry_name);
                return match state.fs.root_fs.symlink_metadata(&child_path) {
                    Ok(metadata) => {
                        if metadata.is_dir() {
                            Err(Errno::Exist)
                        } else {
                            Err(Errno::Notdir)
                        }
                    }
                    Err(err) => Err(fs_error_into_wasi_err(err)),
                };
            }
            Kind::Root { .. } => return Err(Errno::Notcapable),
            _ => return Err(Errno::Notdir),
        }
    }

    // short circuit if anything is wrong, before we create an inode
    {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if entries.contains_key(&entry_name) {
                    return Err(Errno::Exist);
                }
                let mut child_path = path.clone();
                child_path.push(&entry_name);
                match state.fs.root_fs.symlink_metadata(&child_path) {
                    Ok(_) => return Err(Errno::Exist),
                    Err(FsError::EntryNotFound) => {}
                    Err(err) => return Err(fs_error_into_wasi_err(err)),
                }
            }
            Kind::Root { .. } => return Err(Errno::Notcapable),
            Kind::Socket { .. }
            | Kind::PipeRx { .. }
            | Kind::PipeTx { .. }
            | Kind::DuplexPipe { .. }
            | Kind::EventNotifications { .. }
            | Kind::Epoll { .. } => return Err(Errno::Inval),
            Kind::File { .. } | Kind::Symlink { .. } | Kind::Buffer { .. } => {
                return Err(Errno::Notdir);
            }
        }
    }

    let mut source_path = std::path::Path::new(old_path);
    let mut relative_path = std::path::PathBuf::new();
    for _ in 0..depth {
        relative_path.push("..");
    }
    relative_path.push(source_path);

    let kind = Kind::Symlink {
        base_po_dir: fd,
        path_to_symlink: std::path::PathBuf::from(new_path),
        relative_path,
    };
    let new_inode =
        state
            .fs
            .create_inode_with_default_stat(inodes, kind, false, entry_name.clone().into());

    {
        let mut guard = target_parent_inode.write();
        if let Kind::Dir { entries, .. } = guard.deref_mut() {
            entries.insert(entry_name, new_inode);
        }
    }

    Ok(())
}
