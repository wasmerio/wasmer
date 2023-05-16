use super::*;
use crate::syscalls::*;

/// ### `fd_prestat_get()`
/// Get metadata about a preopened file descriptor
/// Input:
/// - `Fd fd`
///     The preopened file descriptor to query
/// Output:
/// - `__wasi_prestat *buf`
///     Where the metadata will be written
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_prestat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Prestat, M>,
) -> Errno {
    let env = ctx.data();
    let (memory, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    let prestat_ptr = buf.deref(&memory);
    wasi_try_mem!(prestat_ptr.write(wasi_try!(state.fs.prestat_fd(fd))));

    Errno::Success
}
