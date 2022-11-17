use super::*;
use crate::syscalls::*;

/// ### `random_get()`
/// Fill buffer with high-quality random data.  This function may be slow and block
/// Inputs:
/// - `void *buf`
///     A pointer to a buffer where the random bytes will be written
/// - `size_t buf_len`
///     The number of bytes that will be written
pub fn random_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
) -> Errno {
    trace!(
        "wasi[{}:{}]::random_get(buf_len={})",
        ctx.data().pid(),
        ctx.data().tid(),
        buf_len
    );
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let buf_len64: u64 = buf_len.into();
    let mut u8_buffer = vec![0; buf_len64 as usize];
    let res = getrandom::getrandom(&mut u8_buffer);
    match res {
        Ok(()) => {
            let buf = wasi_try_mem!(buf.slice(&memory, buf_len));
            wasi_try_mem!(buf.write_slice(&u8_buffer));
            Errno::Success
        }
        Err(_) => Errno::Io,
    }
}
