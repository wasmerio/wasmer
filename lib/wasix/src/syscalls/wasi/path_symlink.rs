use vfs_core::{ResolveFlags, SymlinkOptions, VfsBaseDirAsync, VfsPath};
use vfs_unix::errno::vfs_error_to_wasi_errno;

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
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
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
    let (_memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let base_fd = state.fs.get_fd(fd)?;
    if !base_fd.inner.rights.contains(Rights::PATH_SYMLINK) {
        return Err(Errno::Access);
    }

    let dir_handle = match base_fd.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Err(Errno::Badf),
    };

    let ctx = state.fs.ctx.read().unwrap().clone();
    let old_bytes = old_path.as_bytes().to_vec();
    let new_bytes = new_path.as_bytes().to_vec();
    let res = __asyncify_light(env, None, async move {
        state
            .fs
            .vfs
            .symlinkat_async(
                &ctx,
                VfsBaseDirAsync::Handle(&dir_handle),
                VfsPath::new(&new_bytes),
                VfsPath::new(&old_bytes),
                SymlinkOptions {
                    resolve: ResolveFlags::empty(),
                },
            )
            .await
            .map_err(|err| vfs_error_to_wasi_errno(&err))
    });

    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(Errno::Io),
    }
}
