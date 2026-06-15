use super::*;
use crate::syscalls::*;

const MAX_CHUNK_LEN: usize = 4 * 1024;

/// ### `random_get()`
/// Fill buffer with high-quality random data.  This function may be slow and block
/// Inputs:
/// - `void *buf`
///     A pointer to a buffer where the random bytes will be written
/// - `size_t buf_len`
///     The number of bytes that will be written
#[instrument(level = "trace", skip_all, fields(%buf_len), ret)]
pub fn random_get<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
) -> Errno {
    let buf_len64: u64 = buf_len.into();
    if buf_len64 == 0 {
        return Errno::Success;
    }

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let buf_slice = wasi_try_mem!(buf.slice(&memory, buf_len));
    // If the buffer is owned we cannot call .access() on it
    // as that would trigger unbounded host allocation in JS.
    if buf_slice.is_owned() {
        let mut buffer = vec![0u8; MAX_CHUNK_LEN.min(buf_len64 as usize)];
        let mut offset = 0u64;

        while offset < buf_len64 {
            let chunk_len = usize::min((buf_len64 - offset) as usize, buffer.len());
            if getrandom::fill(&mut buffer[..chunk_len]).is_err() {
                return Errno::Io;
            }

            let chunk = buf_slice.subslice(offset..offset + chunk_len as u64);
            wasi_try_mem!(chunk.write_slice(&buffer[..chunk_len]));

            offset += chunk_len as u64;
        }
    } else {
        let mut buf = wasi_try_mem!(buf_slice.access());
        if getrandom::fill(buf.as_mut()).is_err() {
            return Errno::Io;
        }
    }

    Errno::Success
}
