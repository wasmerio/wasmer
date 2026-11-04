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
#[instrument(level = "trace", skip_all, fields(%fd), ret)]
pub fn fd_fdstat_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    buf_ptr: WasmPtr<Fdstat, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (memory, mut state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    let stat = wasi_try_ok!(state.fs.fdstat(fd));

    let buf = buf_ptr.deref(&memory);

    wasi_try_mem_ok!(buf.write(stat));

    Ok(Errno::Success)
}
