use std::mem::MaybeUninit;

use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `sock_recv_from_poll()`
///
/// Polls for a message and its peer address from a socket.
///
/// Note: This is similar to `recvfrom` in POSIX, though it also supports reading
/// the data into multiple buffers in the manner of `readv` and it will
/// register a waker if no data is available
///
/// ## Parameters
///
/// * `ri_data` - List of scatter/gather vectors to which to store data.
/// * `ri_flags` - Message flags.
/// * `ri_waker` - Waker ID that will be passed back to the program when the waker is triggered
///
/// ## Return
///
/// Number of bytes stored in ri_data and message flags.
#[instrument(level = "trace", skip_all, fields(%sock, nread = field::Empty, peer = field::Empty), ret, err)]
pub fn sock_recv_from_poll<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    ri_flags: RiFlags,
    ri_waker: WakerId,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    // the waker construction needs to be the first line - otherwise errors will leak wakers
    let waker = conv_waker_id(ctx.data().state(), ri_waker);

    sock_recv_from_internal(
        ctx,
        sock,
        ri_data,
        ri_data_len,
        ri_flags,
        ro_data_len,
        ro_flags,
        ro_addr,
        Some(&waker),
    )
}
