use super::*;
use crate::syscalls::*;
use std::collections::HashSet;
use std::path::{Component, PathBuf};
use virtual_fs::{FsError, host_fs::normalize_path};

fn resolve_symlink_path(
    state: &WasiState,
    inodes: &WasiInodes,
    path: &str,
) -> Result<String, Errno> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut current = PathBuf::from(path);

    for _ in 0..MAX_SYMLINKS {
        let inode = match state.fs.get_inode_at_path(
            inodes,
            crate::VIRTUAL_ROOT_FD,
            current.to_string_lossy().as_ref(),
            false,
        ) {
            Ok(inode) => inode,
            Err(_) => return Ok(current.to_string_lossy().into_owned()),
        };

        let guard = inode.read();
        let Kind::Symlink {
            base_po_dir,
            path_to_symlink,
            relative_path,
        } = guard.deref()
        else {
            return Ok(current.to_string_lossy().into_owned());
        };

        if !seen.insert(current.clone()) {
            return Err(Errno::Loop);
        }

        let mut base = path_to_symlink.clone();
        base.pop();
        base.push(relative_path);

        let base_inode = state.fs.get_fd_inode(*base_po_dir)?;
        let base_name = base_inode.name.read().unwrap();
        let mut resolved = Path::new(base_name.as_ref()).to_path_buf();
        resolved.push(base);
        current = normalize_path(resolved.as_path());
    }

    Err(Errno::Loop)
}

/// ### `chdir()`
/// Sets the current working directory
#[instrument(level = "trace", skip_all, fields(name = field::Empty), ret)]
pub fn chdir<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let path = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path.as_str());

    wasi_try_ok!(chdir_internal(ctx.data(), &path));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_chdir(&mut ctx, path).map_err(|err| {
            tracing::error!("failed to chdir event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub fn chdir_internal(env: &WasiEnv, path: &str) -> Result<(), Errno> {
    let state = &env.state;
    let path = state.fs.relative_path_to_absolute(path.to_string());
    let resolved_path = normalize_path(Path::new(path.as_str()))
        .to_string_lossy()
        .into_owned();

    for component in Path::new(resolved_path.as_str()).components() {
        if let Component::Normal(name) = component {
            if name.to_string_lossy().len() > 255 {
                return Err(Errno::Nametoolong);
            }
        }
    }

    let resolved_path = resolve_symlink_path(state, &state.inodes, &resolved_path)?;

    // Check if the directory exists (follow symlinks for the last component)
    match state.fs.get_inode_at_path(
        &state.inodes,
        crate::VIRTUAL_ROOT_FD,
        resolved_path.as_str(),
        true,
    ) {
        Ok(_) => {
            if let Err(err) = state.fs.root_fs.read_dir(Path::new(resolved_path.as_str())) {
                if matches!(err, FsError::PermissionDenied) {
                    return Err(Errno::Access);
                }
            }
            state.fs.set_current_dir(resolved_path.as_str());
            Ok(())
        }
        Err(err) => {
            if err == Errno::Loop || err == Errno::Mlink {
                return Err(err);
            }
            // If it's a symlink, resolve manually and allow chdir.
            if let Ok(symlink_inode) = state.fs.get_inode_at_path(
                &state.inodes,
                crate::VIRTUAL_ROOT_FD,
                &resolved_path,
                false,
            ) {
                let guard = symlink_inode.read();
                if let Kind::Symlink {
                    base_po_dir,
                    path_to_symlink,
                    relative_path,
                } = guard.deref()
                {
                    let mut base = path_to_symlink.clone();
                    base.pop();
                    base.push(relative_path);
                    let base_inode = match state.fs.get_fd_inode(*base_po_dir) {
                        Ok(inode) => inode,
                        Err(_) => return Err(err),
                    };
                    let base_name = base_inode.name.read().unwrap();
                    let mut resolved = Path::new(base_name.as_ref()).to_path_buf();
                    resolved.push(base);
                    let resolved =
                        normalize_path(resolved.as_path()).to_string_lossy().into_owned();
                    state.fs.set_current_dir(resolved.as_str());
                    return Ok(());
                }
            }
            Err(err)
        }
    }
}
