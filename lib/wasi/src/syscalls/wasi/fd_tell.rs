use super::*;
use crate::syscalls::*;

/// ### `fd_tell()`
/// Get the offset of the file descriptor
/// Inputs:
/// - `Fd fd`
///     The file descriptor to access
/// Output:
/// - `Filesize *offset`
///     The offset of `fd` relative to the start of the file
pub fn fd_tell<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    fd: WasiFd,
    offset: WasmPtr<Filesize, M>,
) -> Errno {
    debug!("wasi::fd_tell");
    debug!("wasi[{}:{}]::fd_tell", ctx.data().pid(), ctx.data().tid());
    let env = ctx.data();
    let (memory, mut state) = env.get_memory_and_wasi_state(&ctx, 0);
    let offset_ref = offset.deref(&memory);

    let fd_entry = wasi_try!(state.fs.get_fd(fd));

    if !fd_entry.rights.contains(Rights::FD_TELL) {
        return Errno::Access;
    }

    wasi_try_mem!(offset_ref.write(fd_entry.offset.load(Ordering::Acquire)));

    Errno::Success
}
