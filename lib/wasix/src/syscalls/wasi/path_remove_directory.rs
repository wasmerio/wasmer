use vfs_core::{ResolveFlags, StatOptions, UnlinkOptions, VfsBaseDirAsync, VfsFileType, VfsPath};
use vfs_unix::errno::vfs_error_to_wasi_errno;

use super::*;
use crate::syscalls::*;

/// Returns Errno::Notemtpy if directory is not empty
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_remove_directory<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    // TODO check if fd is a dir, ensure it's within sandbox, etc.
    let env = ctx.data();
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let base_dir = wasi_try_ok!(state.fs.get_fd(fd));
    let path_str = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_str.as_str());

    wasi_try_ok!(path_remove_directory_internal(&mut ctx, fd, &path_str));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        wasi_try_ok!(
            JournalEffector::save_path_remove_directory(&mut ctx, fd, path_str).map_err(|err| {
                tracing::error!("failed to save remove directory event - {}", err);
                Errno::Fault
            })
        )
    }

    Ok(Errno::Success)
}

pub(crate) fn path_remove_directory_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: &str,
) -> Result<(), Errno> {
    let env = ctx.data();
    let (_memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let working_dir = state.fs.get_fd(fd)?;
    if !working_dir
        .inner
        .rights
        .contains(Rights::PATH_REMOVE_DIRECTORY)
    {
        return Err(Errno::Access);
    }

    let dir_handle = match working_dir.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Err(Errno::Badf),
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
                    require_dir_if_trailing_slash: true,
                },
            )
            .await
            .map_err(|err| vfs_error_to_wasi_errno(&err))?;
        if meta.file_type != VfsFileType::Directory {
            return Err(Errno::Notdir);
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
    });

    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(Errno::Io),
    }
}
