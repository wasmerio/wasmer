use super::*;
use crate::syscalls::*;

/// ### `sock_recv_msg()`
/// Receive a message, optional peer address, and optional ancillary data from a
/// socket.
///
/// This is the WASIX backing syscall for POSIX `recvmsg`.
#[instrument(level = "trace", skip_all, fields(%sock, control_capacity = field::Empty), ret)]
pub fn sock_recv_msg<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    ri_flags: RiFlags,
    addr: WasmPtr<__wasi_addr_port_t, M>,
    _ro_control: WasmPtr<u8, M>,
    ro_control_len: M::Offset,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
    ro_control_len_out: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;
    Span::current().record("control_capacity", format!("{ro_control_len:?}"));

    let memory = unsafe { ctx.data().memory_view(&ctx) };
    wasi_try_mem_ok!(ro_control_len_out.write(&memory, M::ZERO));

    if addr.is_null() {
        sock_recv(
            ctx,
            sock,
            ri_data,
            ri_data_len,
            ri_flags,
            ro_data_len,
            ro_flags,
        )
    } else {
        sock_recv_from(
            ctx,
            sock,
            ri_data,
            ri_data_len,
            ri_flags,
            ro_data_len,
            ro_flags,
            addr,
        )
    }
}
