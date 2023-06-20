use std::mem::MaybeUninit;

use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `sock_recv_poll()`
///
/// Polls if a message is waiting to be received
///
#[instrument(level = "trace", skip_all, fields(%sock, nread = field::Empty), ret, err)]
pub fn sock_recv_ready_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_waker: WakerId,
    ro_data_len: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    // the waker construction needs to be the first line - otherwise errors will leak wakers
    let waker = conv_waker_id(ctx.data().state(), ri_waker);

    let pid = ctx.data().pid();
    let tid = ctx.data().tid();

    let amt = wasi_try_ok!(sock_recv_ready_internal::<M>(&mut ctx, sock, &waker)?);

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem_ok!(ro_data_len.write(
        &memory,
        wasi_try_ok!(amt.try_into().map_err(|_| Errno::Overflow))
    ));
    Ok(Errno::Success)
}

pub(super) fn sock_recv_ready_internal<M: MemorySize>(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    waker: &Waker,
) -> Result<Result<usize, Errno>, WasiError> {
    wasi_try_ok_ok!(WasiEnv::process_signals_and_wakes_and_exit(ctx)?);

    let mut env = ctx.data();
    let memory = unsafe { env.memory_view(ctx) };

    let data = wasi_try_ok_ok!(__sock_asyncify(
        env,
        sock,
        Rights::SOCK_RECV,
        Some(waker),
        |socket, fd| async move {
            let mut total_read = 0;
            let local_read = match socket.read_ready().await {
                Ok(s) => s,
                Err(err) => return Err(err),
            };
            Ok(total_read)
        }
    ));
    Ok(Ok(data))
}
