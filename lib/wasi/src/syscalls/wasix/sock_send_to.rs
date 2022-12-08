use super::*;
use crate::syscalls::*;

/// ### `sock_send_to()`
/// Send a message on a socket to a specific address.
/// Note: This is similar to `sendto` in POSIX, though it also supports writing
/// the data from multiple buffers in the manner of `writev`.
///
/// ## Parameters
///
/// * `si_data` - List of scatter/gather vectors to which to retrieve data
/// * `si_flags` - Message flags.
/// * `addr` - Address of the socket to send message to
///
/// ## Return
///
/// Number of bytes transmitted.
pub fn sock_send_to<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    _si_flags: SiFlags,
    addr: WasmPtr<__wasi_addr_port_t, M>,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::sock_send_to (fd={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock
    );

    wasi_try_ok!(ctx.data().clone().process_signals_and_exit(&mut ctx)?);

    let mut env = ctx.data();

    let buf_len: M::Offset = {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));
        iovs_arr
            .iter()
            .filter_map(|a| a.read().ok())
            .map(|a| a.buf_len)
            .sum()
    };
    let buf_len: usize = wasi_try_ok!(buf_len.try_into().map_err(|_| Errno::Inval));
    let mut buf = Vec::with_capacity(buf_len);
    {
        let memory = env.memory_view(&ctx);
        let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));
        wasi_try_ok!(write_bytes(&mut buf, &memory, iovs_arr));
    }

    let (addr_ip, addr_port) = {
        let memory = env.memory_view(&ctx);
        wasi_try_ok!(read_ip_port(&memory, addr))
    };
    let addr = SocketAddr::new(addr_ip, addr_port);

    let bytes_written = wasi_try_ok!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::SOCK_SEND_TO,
        move |socket| async move { socket.send_to::<M>(buf, addr).await },
    )?);
    env = ctx.data();

    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    let memory = env.memory_view(&ctx);
    wasi_try_mem_ok!(ret_data_len.write(&memory, bytes_written as M::Offset));

    Ok(Errno::Success)
}
