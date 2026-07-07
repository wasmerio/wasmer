use super::*;
use crate::syscalls::*;

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

    wasi_try_ok!(__asyncify_light(
        env,
        None,
        path_symlink_internal(env, &old_path_str, fd, &new_path_str)
    )?);
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

pub async fn path_symlink_internal(
    env: &WasiEnv,
    old_path: &str,
    fd: WasiFd,
    new_path: &str,
) -> Result<(), Errno> {
    let state = env.state();
    let inodes = &state.inodes;

    let base_fd = state.fs.get_fd(fd)?;
    if !base_fd.inner.rights.contains(Rights::PATH_SYMLINK) {
        return Err(Errno::Access);
    }

    let new_path_path = std::path::Path::new(new_path);
    let (target_parent_inode, entry_name) =
        state
            .fs
            .get_parent_inode_at_path(inodes, fd, new_path_path, true)
            .await?;

    let symlink_path = {
        let guard = target_parent_inode.read();
        match guard.deref() {
            Kind::Dir { entries, path, .. } => {
                if entries.contains_key(&entry_name) {
                    return Err(Errno::Exist);
                }
                crate::fs::PosixPath::from_path(path)
                    .join(&crate::fs::PosixPath::new(&entry_name))
                    .into_path_buf()
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
    };

    // Guest-created symlinks live in the virtual filesystem namespace. Keep
    // their location relative to the virtual root so targets like
    // `/temp/link -> ../hamlet/file` can cross sibling preopens without
    // escaping the guest sandbox.
    let path_to_symlink = crate::fs::PosixPath::from_path(&symlink_path)
        .strip_root_prefix()
        .into_path_buf();
    let relative_path = std::path::PathBuf::from(old_path);

    let source_path = std::path::Path::new(old_path);
    let target_path = symlink_path.as_path();
    let persisted_in_backing_fs = state
        .fs
        .root_fs
        .create_symlink(source_path, target_path)
        .await;

    let needs_ephemeral_fallback = match persisted_in_backing_fs {
        Ok(()) => false,
        Err(virtual_fs::FsError::Unsupported) => true,
        Err(err) => return Err(fs_error_into_wasi_err(err)),
    };

    let kind = Kind::Symlink {
        symlink_kind: crate::fs::SymlinkKind::Virtual,
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

    // Keep transient map in sync with the backing outcome.
    if needs_ephemeral_fallback {
        state
            .fs
            .register_ephemeral_symlink(symlink_path, path_to_symlink, relative_path);
    } else {
        state.fs.unregister_ephemeral_symlink(target_path);
    }

    Ok(())
}
