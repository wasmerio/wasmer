use vfs_core::{RenameFlags, RenameOptions, ResolveFlags, VfsBaseDirAsync, VfsPath};
use vfs_unix::errno::vfs_error_to_wasi_errno;

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
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
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
    let (_memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let source_fd_entry = wasi_try_ok!(state.fs.get_fd(source_fd));
    if !source_fd_entry
        .inner
        .rights
        .contains(Rights::PATH_RENAME_SOURCE)
    {
        return Ok(Errno::Access);
    }
    let target_fd_entry = wasi_try_ok!(state.fs.get_fd(target_fd));
    if !target_fd_entry
        .inner
        .rights
        .contains(Rights::PATH_RENAME_TARGET)
    {
        return Ok(Errno::Access);
    }

    let source_dir = match source_fd_entry.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Ok(Errno::Badf),
    };
    let target_dir = match target_fd_entry.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Ok(Errno::Badf),
    };

    let ctx = state.fs.ctx.read().unwrap().clone();
    let source_bytes = source_path.as_bytes().to_vec();
    let target_bytes = target_path.as_bytes().to_vec();
    let res = __asyncify_light(env, None, async move {
        state
            .fs
            .vfs
            .renameat_async(
                &ctx,
                VfsBaseDirAsync::Handle(&source_dir),
                VfsPath::new(&source_bytes),
                VfsBaseDirAsync::Handle(&target_dir),
                VfsPath::new(&target_bytes),
                RenameOptions {
                    flags: RenameFlags::empty(),
                    resolve: ResolveFlags::empty(),
                },
            )
            .await
            .map_err(|err| vfs_error_to_wasi_errno(&err))
    })?;

    match res {
        Ok(()) => Ok(Errno::Success),
        Err(err) => Ok(err),
    }
}
