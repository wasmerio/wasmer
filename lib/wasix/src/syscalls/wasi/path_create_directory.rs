use vfs_core::{MkdirOptions, ResolveFlags, VfsBaseDirAsync, VfsPath};
use vfs_unix::errno::vfs_error_to_wasi_errno;

use super::*;
use crate::syscalls::*;

/// ### `path_create_directory()`
/// Create directory at a path
/// Inputs:
/// - `Fd fd`
///     The directory that the path is relative to
/// - `const char *path`
///     String containing path data
/// - `u32 path_len`
///     The length of `path`
/// Errors:
/// Required Rights:
/// - Rights::PATH_CREATE_DIRECTORY
///     This right must be set on the directory that the file is created in (TODO: verify that this is true)
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_create_directory<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, _state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let mut path_string = unsafe { get_input_str_ok!(&memory, path, path_len) };
    Span::current().record("path", path_string.as_str());

    wasi_try_ok!(path_create_directory_internal(&mut ctx, fd, &path_string));
    let env = ctx.data();

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_path_create_directory(&mut ctx, fd, path_string).map_err(|err| {
            tracing::error!("failed to save create directory event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn path_create_directory_internal(
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
        .contains(Rights::PATH_CREATE_DIRECTORY)
    {
        trace!("working directory (fd={fd}) has no rights to create a directory");
        return Err(Errno::Access);
    }

    let dir_handle = match working_dir.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Err(Errno::Badf),
    };

    let ctx = state.fs.ctx.read().unwrap().clone();
    let path_bytes = path.as_bytes().to_vec();
    let res = __asyncify_light(env, None, async move {
        state
            .fs
            .vfs
            .mkdirat_async(
                &ctx,
                VfsBaseDirAsync::Handle(&dir_handle),
                VfsPath::new(&path_bytes),
                MkdirOptions {
                    mode: None,
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
