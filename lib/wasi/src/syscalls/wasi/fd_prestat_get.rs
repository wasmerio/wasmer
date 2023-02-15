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
pub fn fd_prestat_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf: WasmPtr<Prestat, M>,
) -> Errno {
    trace!(
        "wasi[{}:{}]::fd_prestat_get: fd={}",
        ctx.data().pid(),
        ctx.data().tid(),
        fd
    );
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);

    let prestat_ptr = buf.deref(&memory);
    wasi_try_mem!(
        prestat_ptr.write(wasi_try!(state.fs.prestat_fd(fd).map_err(|code| {
            debug!("fd_prestat_get failed (fd={}) - errno={}", fd, code);
            code
        })))
    );

    Errno::Success
}
