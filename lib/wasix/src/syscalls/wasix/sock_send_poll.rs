use std::mem::MaybeUninit;

use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `sock_send_poll()`
///
/// Polls to send a message on a socket.
///
/// Note: This is similar to `send` in POSIX, though it also supports writing
/// the data from multiple buffers in the manner of `writev`  nd it will
/// register a waker for when the socket can send data again
///
/// ## Parameters
///
/// * `si_data` - List of scatter/gather vectors to which to retrieve data
/// * `si_flags` - Message flags.
/// * `si_waker` - Waker ID that will be passed back to the program when the waker is triggered
///
/// ## Return
///
/// Number of bytes transmitted.
#[instrument(level = "trace", skip_all, fields(%sock, nsent = field::Empty), ret, err)]
pub fn sock_send_poll<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    si_data: WasmPtr<__wasi_ciovec_t<M>, M>,
    si_data_len: M::Offset,
    si_flags: SiFlags,
    si_waker: WakerId,
    ret_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    // the waker construction needs to be the first line - otherwise errors will leak wakers
    let waker = conv_waker_id(ctx.data().state(), si_waker);

    sock_send_internal(
        ctx,
        sock,
        si_data,
        si_data_len,
        si_flags,
        ret_data_len,
        Some(&waker),
    )
}
