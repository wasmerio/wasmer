use vfs_core::{ResolveFlags, StatOptions, UnlinkOptions, VfsBaseDirAsync, VfsFileType, VfsPath};
use vfs_unix::errno::vfs_error_to_wasi_errno;

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
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let base_dir = wasi_try_ok!(state.fs.get_fd(fd));
    if !base_dir.inner.rights.contains(Rights::PATH_UNLINK_FILE) {
        return Ok(Errno::Access);
    }
    let path_str = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    let ret = path_unlink_file_internal(&mut ctx, fd, &path_str)?;
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

pub(crate) fn path_unlink_file_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: &str,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let (_memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let base_dir = state.fs.get_fd(fd)?;
    if !base_dir.inner.rights.contains(Rights::PATH_UNLINK_FILE) {
        return Ok(Errno::Access);
    }

    let dir_handle = match base_dir.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Ok(Errno::Badf),
    };

    let ctx = state.fs.ctx.read().unwrap().clone();
    let path_bytes = path.as_bytes().to_vec();
    let res = __asyncify_light(env, None, async move {
        let base = VfsBaseDirAsync::Handle(&dir_handle);
        let vfs_path = VfsPath::new(&path_bytes);
        let meta = state
            .fs
            .vfs
            .statat_async(
                &ctx,
                base,
                vfs_path,
                StatOptions {
                    resolve: ResolveFlags::empty(),
                    follow: true,
                    require_dir_if_trailing_slash: false,
                },
            )
            .await
            .map_err(|err| vfs_error_to_wasi_errno(&err))?;
        if meta.file_type == VfsFileType::Directory {
            return Err(Errno::Isdir);
        }
        state
            .fs
            .vfs
            .unlinkat_async(
                &ctx,
                base,
                vfs_path,
                UnlinkOptions {
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
