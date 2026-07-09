use super::*;
use crate::syscalls::*;

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

    wasi_try_ok!(__asyncify_light(
        env,
        None,
        chdir_internal(ctx.data(), &path)
    )?);
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

/// Change the WASIX current directory to the directory reached by resolving
/// `path`.
///
/// `chdir` is intentionally not a textual path update. POSIX shells often keep
/// a logical `$PWD` for display, so after `cd some_symlink` a shell builtin
/// `pwd` may print the symlinked spelling. The process current directory,
/// though, is the resolved directory object. A physical lookup such as
/// `realpath(".")` observes the symlink target. WASIX stores the latter form in
/// `state.fs.current_dir`, because this string is used by the runtime to resolve
/// future relative paths, not merely to echo the spelling the user typed.
///
/// The resolver is called with `follow_symlinks = true`, matching `chdir(2)`:
/// the final component must be followed if it is a symlink, and the resolved
/// inode must be a directory. The stored cwd therefore comes from the resolved
/// `Kind::Dir` path (or `/` for the virtual root), not from the original input
/// path or its lexical absolute form. Preserving the logical input here would
/// make later relative lookups re-enter symlink paths and would diverge from the
/// directory object that `chdir` actually selected.
///
/// For resolved non-root directories, the final `read_dir` check refreshes the
/// backing filesystem view enough to reject stale cached directories that can no
/// longer be listed.
pub async fn chdir_internal(env: &WasiEnv, path: &str) -> Result<(), Errno> {
    let state = &env.state;
    if path.is_empty() {
        return Err(Errno::Noent);
    }

    let path = state.fs.relative_path_to_absolute(path.to_string());
    let inode = state
        .fs
        .get_inode_at_path(&state.inodes, crate::VIRTUAL_ROOT_FD, &path, true)
        .await?;

    let resolved_path = {
        let guard = inode.read();
        match guard.deref() {
            Kind::Dir { path, .. } => crate::fs::PosixPath::from_path(path).as_str().to_owned(),
            Kind::Root { .. } => "/".to_string(),
            _ => return Err(Errno::Notdir),
        }
    };
    if resolved_path != "/"
        && state
            .fs
            .root_fs
            .read_dir(Path::new(&resolved_path))
            .await
            .is_err()
    {
        return Err(Errno::Noent);
    }

    state.fs.set_current_dir(&resolved_path);
    Ok(())
}
