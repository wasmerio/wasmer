use super::*;
use crate::syscalls::*;
use virtual_fs::host_fs::normalize_path;

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

    // Check if the directory exists (follow symlinks for the last component)
    match state.fs.get_inode_at_path(
        &state.inodes,
        crate::VIRTUAL_ROOT_FD,
        &resolved_path,
        true,
    ) {
        Ok(_) => {
            state.fs.set_current_dir(resolved_path.as_str());
            Ok(())
        }
        Err(err) => {
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
