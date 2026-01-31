use vfs_core::{ResolveFlags, StatOptions, VfsBaseDirAsync, VfsPath};
use vfs_unix::{errno::vfs_error_to_wasi_errno, vfs_filetype_to_wasi};

use super::*;
use crate::syscalls::*;
use crate::types::wasi::Snapshot0Filestat;

/// ### `path_filestat_get()`
/// Access metadata about a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory that `path` is relative to
/// - `LookupFlags flags`
///     Flags to control how `path` is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// Output:
/// - `__wasi_file_stat_t *buf`
///     The location where the metadata will be stored
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_filestat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let mut path_string = unsafe { get_input_str!(&memory, path, path_len) };

    // Convert relative paths into absolute paths
    // let path_str = if path_string == "." {
    //     &"/"
    // } else if path_string.starts_with("./") {
    //     &path_string[1..]
    // } else {
    //     &path_string
    // };
    // if path_string.starts_with("./") || path_string == "." {
    //     path_string = ctx.data().state.fs.relative_path_to_absolute(path_string);
    // }
    tracing::trace!(path = path_string.as_str());

    let stat = wasi_try!(path_filestat_get_internal(
        env,
        state,
        fd,
        flags,
        &path_string
    ));

    wasi_try_mem!(buf.deref(&memory).write(stat));

    Errno::Success
}

/// ### `path_filestat_get_internal()`
/// return a Filstat or Errno
pub(crate) fn path_filestat_get_internal(
    env: &WasiEnv,
    state: &WasiState,
    fd: WasiFd,
    flags: LookupFlags,
    path_string: &str,
) -> Result<Filestat, Errno> {
    let root_dir = state.fs.get_fd(fd)?;
    if !root_dir.inner.rights.contains(Rights::PATH_FILESTAT_GET) {
        return Err(Errno::Access);
    }

    let dir_handle = match root_dir.kind {
        Kind::VfsDir { handle } => handle,
        _ => return Err(Errno::Badf),
    };

    let resolve = if (flags & __WASI_LOOKUP_SYMLINK_FOLLOW) != 0 {
        ResolveFlags::empty()
    } else {
        ResolveFlags::NO_SYMLINK_FOLLOW
    };
    let options = StatOptions {
        resolve,
        follow: (flags & __WASI_LOOKUP_SYMLINK_FOLLOW) != 0,
        require_dir_if_trailing_slash: path_string.ends_with('/'),
    };
    let path_bytes = path_string.as_bytes().to_vec();
    let ctx = state.fs.ctx.read().unwrap().clone();
    let meta = __asyncify_light(env, None, async move {
        state
            .fs
            .vfs
            .statat_async(
                &ctx,
                VfsBaseDirAsync::Handle(&dir_handle),
                VfsPath::new(&path_bytes),
                options,
            )
            .await
            .map_err(|err| vfs_error_to_wasi_errno(&err))
    });

    let timespec_to_timestamp = |ts: vfs_core::VfsTimespec| -> Timestamp {
        if ts.secs < 0 {
            0
        } else {
            (ts.secs as u64)
                .saturating_mul(1_000_000_000)
                .saturating_add(ts.nanos as u64)
        }
    };

    let meta = match meta {
        Ok(Ok(meta)) => meta,
        Ok(Err(err)) => return Err(err),
        Err(_) => return Err(Errno::Io),
    };

    Ok(Filestat {
        st_dev: 0,
        st_ino: meta.inode.backend.get(),
        st_filetype: vfs_filetype_to_wasi(meta.file_type),
        st_nlink: meta.nlink,
        st_size: meta.size,
        st_atim: timespec_to_timestamp(meta.atime),
        st_mtim: timespec_to_timestamp(meta.mtime),
        st_ctim: timespec_to_timestamp(meta.ctime),
    })
}

/// ### `path_filestat_get_old()`
/// Access metadata about a file or directory
/// Inputs:
/// - `Fd fd`
///     The directory that `path` is relative to
/// - `LookupFlags flags`
///     Flags to control how `path` is understood
/// - `const char *path`
///     String containing the file path
/// - `u32 path_len`
///     The length of the `path` string
/// Output:
/// - `__wasi_file_stat_t *buf`
///     The location where the metadata will be stored
#[instrument(level = "trace", skip_all, fields(%fd, path = field::Empty), ret)]
pub fn path_filestat_get_old<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    flags: LookupFlags,
    path: WasmPtr<u8, M>,
    path_len: M::Offset,
    buf: WasmPtr<Snapshot0Filestat, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let path_string = unsafe { get_input_str!(&memory, path, path_len) };
    Span::current().record("path", path_string.as_str());

    let stat = wasi_try!(path_filestat_get_internal(
        env,
        state,
        fd,
        flags,
        &path_string
    ));

    let old_stat = Snapshot0Filestat {
        st_dev: stat.st_dev,
        st_ino: stat.st_ino,
        st_filetype: stat.st_filetype,
        st_nlink: stat.st_nlink as u32,
        st_size: stat.st_size,
        st_atim: stat.st_atim,
        st_mtim: stat.st_mtim,
        st_ctim: stat.st_ctim,
    };
    wasi_try_mem!(buf.deref(&memory).write(old_stat));

    Errno::Success
}
