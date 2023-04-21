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
#[instrument(level = "trace", skip_all, fields(%sock, ?addr, nsent = field::Empty), ret, err)]
pub fn sock_send_to<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    _si_flags: SiFlags,
    addr: WasmPtr<__wasi_addr_port_t, M>,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let iovs_arr = wasi_try_mem_ok!(si_data.slice(&memory, si_data_len));

    let (addr_ip, addr_port) = {
        let memory = env.memory_view(&ctx);
        wasi_try_ok!(read_ip_port(&memory, addr))
    };
    let addr = SocketAddr::new(addr_ip, addr_port);
    Span::current().record("addr", &format!("{:?}", addr));

    let bytes_written = {
        wasi_try_ok!(__sock_asyncify(
            env,
            sock,
            Rights::SOCK_SEND_TO,
            |socket, fd| async move {
                let iovs_arr = si_data
                    .slice(&memory, si_data_len)
                    .map_err(mem_error_to_wasi)?;
                let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;

                let mut sent = 0usize;
                for iovs in iovs_arr.iter() {
                    let buf = WasmPtr::<u8, M>::new(iovs.buf)
                        .slice(&memory, iovs.buf_len)
                        .map_err(mem_error_to_wasi)?
                        .access()
                        .map_err(mem_error_to_wasi)?;
                    let local_sent = match socket
                        .send_to::<M>(env.tasks().deref(), buf.as_ref(), addr, fd.flags)
                        .await
                    {
                        Ok(s) => s,
                        Err(_) if sent > 0 => break,
                        Err(err) => return Err(err),
                    };
                    sent += local_sent;
                    if local_sent != buf.len() {
                        break;
                    }
                }
                Ok(sent)
            },
        ))
    };
    Span::current().record("nsent", bytes_written);

    let memory = env.memory_view(&ctx);
    let bytes_written: M::Offset =
        wasi_try_ok!(bytes_written.try_into().map_err(|_| Errno::Overflow));
    wasi_try_mem_ok!(ret_data_len.write(&memory, bytes_written));

    Ok(Errno::Success)
}
