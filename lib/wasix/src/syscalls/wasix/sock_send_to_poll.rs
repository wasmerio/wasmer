use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `sock_send_to_poll()`
///
/// Polls to send a message on a socket to a specific address.
///
/// Note: This is similar to `sendto` in POSIX, though it also supports writing
/// the data from multiple buffers in the manner of `writev` and it will
/// register a waker for when space is available to send
///
/// ## Parameters
///
/// * `si_data` - List of scatter/gather vectors to which to retrieve data
/// * `si_flags` - Message flags.
/// * `si_waker` - Waker ID that will be passed back to the program when the waker is triggered
/// * `addr` - Address of the socket to send message to
///
/// ## Return
///
/// Number of bytes transmitted.
#[instrument(level = "trace", skip_all, fields(%sock, ?addr, nsent = field::Empty), ret, err)]
pub fn sock_send_to_poll<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    si_flags: SiFlags,
    si_waker: WakerId,
    addr: WasmPtr<__wasi_addr_port_t, M>,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    let waker = conv_waker_id(ctx.data().state(), si_waker);
    sock_send_to_internal(
        ctx,
        sock,
        si_data,
        si_data_len,
        si_flags,
        addr,
        ret_data_len,
        Some(&waker),
    )
}
