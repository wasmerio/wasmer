use super::*;
use crate::syscalls::*;

/// ### `fd_fdstat_get()`
/// Get metadata of a file descriptor
/// Input:
/// - `Fd fd`
///     The file descriptor whose metadata will be accessed
/// Output:
/// - `Fdstat *buf`
///     The location where the metadata will be written
#[instrument(level = "trace", skip_all, fields(%wasi_fd), ret)]
pub fn fd_fdflags_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    wasi_fd: WasiFd,
    buf_ptr: WasmPtr<Fdflagsext, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd = wasi_try!(state.fs.get_fd(wasi_fd));

    let buf = buf_ptr.deref(&memory);

    wasi_try_mem!(buf.write(fd.inner.fd_flags));

    Errno::Success
}
