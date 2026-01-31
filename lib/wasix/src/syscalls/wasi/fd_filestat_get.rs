use vfs_unix::{errno::vfs_error_to_wasi_errno, vfs_filetype_to_wasi};

use super::*;
use crate::syscalls::*;
use crate::types::wasi::Snapshot0Filestat;

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
///
/// Input:
/// - `Fd fd`
///   The open file descriptor whose metadata will be read
///
/// Output:
/// - `Filestat *buf`
///   Where the metadata from `fd` will be written
#[instrument(level = "trace", skip_all, fields(%fd, size = field::Empty, mtime = field::Empty), ret)]
pub fn fd_filestat_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Filestat, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let stat = wasi_try_ok!(fd_filestat_get_internal(&mut ctx, fd));

    // These two values have proved to be helpful in multiple investigations
    Span::current().record("size", stat.st_size);
    Span::current().record("mtime", stat.st_mtim);

    let env = ctx.data();
    let (memory, _) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let buf = buf.deref(&memory);
    wasi_try_mem_ok!(buf.write(stat));

    Ok(Errno::Success)
}

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
///
/// Input:
/// - `__wasi_fd_t fd`
///   The open file descriptor whose metadata will be read
///
/// Output:
/// - `__wasi_filestat_t *buf`
///   Where the metadata from `fd` will be written
pub(crate) fn fd_filestat_get_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
) -> Result<Filestat, Errno> {
    let env = ctx.data();
    let (_, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = state.fs.get_fd(fd)?;
    if !fd_entry.inner.rights.contains(Rights::FD_FILESTAT_GET) {
        return Err(Errno::Access);
    }

    let timespec_to_timestamp = |ts: vfs_core::VfsTimespec| -> Timestamp {
        if ts.secs < 0 {
            0
        } else {
            (ts.secs as u64)
                .saturating_mul(1_000_000_000)
                .saturating_add(ts.nanos as u64)
        }
    };

    match fd_entry.kind {
        Kind::VfsFile { handle } => {
            let handle = handle.clone();
            let meta = __asyncify_light(env, None, async move {
                handle
                    .get_metadata()
                    .await
                    .map_err(|err| vfs_error_to_wasi_errno(&err))
            })
            .map_err(|_| Errno::Io)?
            .map_err(|err| err)?;

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
        Kind::VfsDir { handle } => {
            let handle = handle.clone();
            let meta = __asyncify_light(env, None, async move {
                handle
                    .node()
                    .metadata()
                    .await
                    .map_err(|err| vfs_error_to_wasi_errno(&err))
            })
            .map_err(|_| Errno::Io)?
            .map_err(|err| err)?;

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
        Kind::Stdin { .. } | Kind::Stdout { .. } | Kind::Stderr { .. } => Ok(Filestat {
            st_dev: 0,
            st_ino: 0,
            st_filetype: Filetype::CharacterDevice,
            st_nlink: 1,
            st_size: 0,
            st_atim: 0,
            st_mtim: 0,
            st_ctim: 0,
        }),
        Kind::Socket { .. } => Ok(Filestat {
            st_dev: 0,
            st_ino: 0,
            st_filetype: Filetype::SocketStream,
            st_nlink: 1,
            st_size: 0,
            st_atim: 0,
            st_mtim: 0,
            st_ctim: 0,
        }),
        Kind::PipeRx { .. } | Kind::PipeTx { .. } | Kind::DuplexPipe { .. } => Ok(Filestat {
            st_dev: 0,
            st_ino: 0,
            st_filetype: Filetype::Unknown,
            st_nlink: 1,
            st_size: 0,
            st_atim: 0,
            st_mtim: 0,
            st_ctim: 0,
        }),
        Kind::EventNotifications { .. } | Kind::Epoll { .. } => Ok(Filestat {
            st_dev: 0,
            st_ino: 0,
            st_filetype: Filetype::Unknown,
            st_nlink: 1,
            st_size: 0,
            st_atim: 0,
            st_mtim: 0,
            st_ctim: 0,
        }),
        Kind::Buffer { buffer } => Ok(Filestat {
            st_dev: 0,
            st_ino: 0,
            st_filetype: Filetype::RegularFile,
            st_nlink: 1,
            st_size: buffer.len() as u64,
            st_atim: 0,
            st_mtim: 0,
            st_ctim: 0,
        }),
    }
}

/// ### `fd_filestat_get_old()`
/// Get the metadata of an open file
///
/// Input:
/// - `Fd fd`
///   The open file descriptor whose metadata will be read
///
/// Output:
/// - `Snapshot0Filestat *buf`
///   Where the metadata from `fd` will be written
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_filestat_get_old<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Snapshot0Filestat, M>,
) -> Errno {
    let stat = wasi_try!(fd_filestat_get_internal(&mut ctx, fd));

    let env = ctx.data();
    let (memory, _) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
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

    let buf = buf.deref(&memory);
    wasi_try_mem!(buf.write(old_stat));

    Errno::Success
}
