use std::mem::MaybeUninit;

use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `sock_recv_poll()`
///
/// Polls for a message from a socket.
///
/// Note: This is similar to `recv` in POSIX, though it also supports reading
/// the data into multiple buffers in the manner of `readv` and it will
/// register a waker if no data is available
///
/// ## Parameters
///
/// * `ri_data` - List of scatter/gather vectors to which to store data.
/// * `ri_flags` - Message flags.
/// * `ri_waker` - Waker that will be invoked when data arrives
///
/// ## Return
///
/// Number of bytes stored in ri_data and message flags.
#[instrument(level = "trace", skip_all, fields(%sock, nread = field::Empty), ret, err)]
pub fn sock_recv_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_data: WasmPtr<__wasi_iovec_t<M>, M>,
    ri_data_len: M::Offset,
    ri_flags: RiFlags,
    ri_waker: WakerId,
    ro_data_len: WasmPtr<M::Offset, M>,
    ro_flags: WasmPtr<RoFlags, M>,
) -> Result<Errno, WasiError> {
    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    let waker = conv_waker_id(ctx.data().state(), ri_waker);
    let res = sock_recv_internal::<M>(
        &mut ctx,
        sock,
        ri_data,
        ri_data_len,
        ri_flags,
        ro_data_len,
        ro_flags,
        Some(&waker),
    )?;

    sock_recv_internal_handler(ctx, res, ro_data_len, ro_flags)
}
