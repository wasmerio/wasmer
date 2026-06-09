use super::*;
use crate::syscalls::*;

/// ### `sock_send_msg()`
/// Send a message, optional peer address, and optional ancillary data on a
/// socket.
///
/// This is the WASIX backing syscall for POSIX `sendmsg`.
#[instrument(level = "trace", skip_all, fields(%sock, control_len = field::Empty), ret)]
pub fn sock_send_msg<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    si_flags: SiFlags,
    addr: WasmPtr<__wasi_addr_port_t, M>,
    si_control: WasmPtr<u8, M>,
    si_control_len: M::Offset,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;
    Span::current().record("control_len", format!("{si_control_len:?}"));

    if !si_control.is_null() && si_control_len != M::ZERO {
        return Ok(Errno::Notsup);
    }

    if addr.is_null() {
        sock_send(ctx, sock, si_data, si_data_len, si_flags, ret_data_len)
    } else {
        sock_send_to(
            ctx,
            sock,
            si_data,
            si_data_len,
            si_flags,
            addr,
            ret_data_len,
        )
    }
}
