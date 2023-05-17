use super::*;
use crate::syscalls::*;
use crate::types::wasi::Snapshot0Filestat;

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `Fd fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `Filestat *buf`
///     Where the metadata from `fd` will be written
#[instrument(level = "debug", skip_all, fields(%fd), ret)]
pub fn fd_filestat_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    let stat = wasi_try!(fd_filestat_get_internal(&mut ctx, fd));

    let env = ctx.data();
    let (memory, _) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let buf = buf.deref(&memory);
    wasi_try_mem!(buf.write(stat));

    Errno::Success
}

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `__wasi_fd_t fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `__wasi_filestat_t *buf`
///     Where the metadata from `fd` will be written
pub(crate) fn fd_filestat_get_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
) -> Result<Filestat, Errno> {
    let env = ctx.data();
    let (_, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let fd_entry = state.fs.get_fd(fd)?;
    if !fd_entry.rights.contains(Rights::FD_FILESTAT_GET) {
        return Err(Errno::Access);
    }

    state.fs.filestat_fd(fd)
}

/// ### `fd_filestat_get_old()`
/// Get the metadata of an open file
/// Input:
/// - `Fd fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `Snapshot0Filestat *buf`
///     Where the metadata from `fd` will be written
#[instrument(level = "debug", skip_all, fields(%fd), ret)]
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
