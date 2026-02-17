use super::*;
use crate::fs::WasiFsRoot;
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

    let new_path_path = std::path::Path::new(new_path);
    let (target_parent_inode, entry_name) =
        state
            .fs
            .get_parent_inode_at_path(inodes, fd, new_path_path, true)?;

    let symlink_path = {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { path, .. } => {
                let mut symlink_path = path.clone();
                symlink_path.push(&entry_name);
                symlink_path
            }
            Kind::Root { .. } => {
                let mut symlink_path = std::path::PathBuf::from("/");
                symlink_path.push(&entry_name);
                symlink_path
            }
            _ => unreachable!("parent inode should be a directory"),
        }
    };

    // Resolve symlink location to (preopen fd, relative path within that preopen)
    // so runtime-created symlinks behave the same as symlinks discovered via readlink().
    let (base_po_dir, path_to_symlink) = state
        .fs
        .path_into_pre_open_and_relative_path_owned(&symlink_path)?;
    let relative_path = std::path::PathBuf::from(old_path);

    // short circuit if anything is wrong, before we create an inode
    {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { entries, .. } => {
                if entries.contains_key(&entry_name) {
                    return Err(Errno::Exist);
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
                unreachable!("get_parent_inode_at_path returned something other than a Dir or Root")
            }
        }
    }

    let kind = Kind::Symlink {
        base_po_dir,
        path_to_symlink: path_to_symlink.clone(),
        relative_path: relative_path.clone(),
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

    let source_path = std::path::Path::new(old_path);
    let target_path = symlink_path.as_path();
    let persisted_in_backing_fs = match &state.fs.root_fs {
        WasiFsRoot::Sandbox(fs) => fs.create_symlink(source_path, target_path),
        WasiFsRoot::Overlay(overlay) => overlay.primary().create_symlink(source_path, target_path),
        WasiFsRoot::Backing(_) => Err(FsError::Unsupported),
    };

    // Fallback registry for environments/filesystems that don't have native
    // symlink creation support in the backing implementation.
    match persisted_in_backing_fs {
        Ok(()) => {}
        Err(FsError::Unsupported) => {
            state.fs.register_ephemeral_symlink(
                symlink_path.clone(),
                base_po_dir,
                path_to_symlink,
                relative_path,
            );
        }
        Err(err) => return Err(fs_error_into_wasi_err(err)),
    }

    Ok(())
}
