use super::*;
use crate::syscalls::*;

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `Fd fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `Filestat *buf`
///     Where the metadata from `fd` will be written
pub fn fd_filestat_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    fd_filestat_get_internal(&mut ctx, fd, buf)
}

/// ### `fd_filestat_get()`
/// Get the metadata of an open file
/// Input:
/// - `__wasi_fd_t fd`
///     The open file descriptor whose metadata will be read
/// Output:
/// - `__wasi_filestat_t *buf`
///     Where the metadata from `fd` will be written
pub(crate) fn fd_filestat_get_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Filestat, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::fd_filestat_get: fd={fd}",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let (memory, mut state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);
    let fd_entry = wasi_try!(state.fs.get_fd(fd));
    if !fd_entry.rights.contains(Rights::FD_FILESTAT_GET) {
        return Errno::Access;
    }

    let stat = wasi_try!(state.fs.filestat_fd(fd));

    let buf = buf.deref(&memory);
    wasi_try_mem!(buf.write(stat));

    Errno::Success
}
