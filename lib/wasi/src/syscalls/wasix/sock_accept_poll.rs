use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `sock_accept_poll()`
/// Polls to accept a new incoming connection. Will also register
/// a waker when a connection is waiting
///
/// Note: This is similar to `accept` in POSIX.
///
/// ## Parameters
///
/// * `fd` - The listening socket.
/// * `flags` - The desired values of the file descriptor flags.
/// * `ri_waker` - Waker ID that will be passed back to the program when the waker is triggered
///
/// ## Return
///
/// New socket connection
#[instrument(level = "debug", skip_all, fields(%sock, fd = field::Empty), ret, err)]
pub fn sock_accept_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    mut fd_flags: Fdflags,
    ri_waker: WakerId,
    ro_fd: WasmPtr<WasiFd, M>,
    ro_addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    let waker = conv_waker_id(ctx.data().state(), ri_waker);
    sock_accept_internal(ctx, sock, fd_flags, ro_fd, ro_addr, Some(&waker))
}
