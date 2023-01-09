use super::*;
use crate::syscalls::*;

/// ### `sock_recv()`
/// Receive a message from a socket.
/// Note: This is similar to `recv` in POSIX, though it also supports reading
/// the data into multiple buffers in the manner of `readv`.
///
/// ## Parameters
///
/// * `ri_data` - List of scatter/gather vectors to which to store data.
/// * `ri_flags` - Message flags.
///
/// ## Return
///
/// Number of bytes stored in ri_data and message flags.
pub fn sock_recv<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    _ri_flags: RiFlags,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    let mut env = ctx.data();

    let max_size = {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));
        let mut max_size = 0usize;
        for iovs in iovs_arr.iter() {
            let iovs = wasi_try_mem_ok!(iovs.read());
            let buf_len: usize = wasi_try_ok!(iovs.buf_len.try_into().map_err(|_| Errno::Overflow));
            max_size += buf_len;
        }
        max_size
    };

    let data = wasi_try_ok!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_RECV,
        move |socket| async move { socket.recv(max_size).await },
    ));
    env = ctx.data();

    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(ri_data.slice(&memory, ri_data_len));

    let data_len = data.len();
    let mut reader = &data[..];
    let bytes_read = wasi_try_ok!(read_bytes(reader, &memory, iovs_arr).map(|_| data_len));

    debug!(
        "wasi[{}:{}]::sock_recv (fd={}, read={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        bytes_read,
    );

    let bytes_read: M::Offset = wasi_try_ok!(bytes_read.try_into().map_err(|_| Errno::Overflow));

    wasi_try_mem_ok!(ro_flags.write(&memory, 0));
    wasi_try_mem_ok!(ro_data_len.write(&memory, bytes_read));

    Ok(Errno::Success)
}
