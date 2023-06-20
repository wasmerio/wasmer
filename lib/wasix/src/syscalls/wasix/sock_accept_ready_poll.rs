use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `sock_accept_ready_poll()`
///
/// Polls to see if new incoming connection are waiting
#[instrument(level = "debug", skip_all, fields(%sock, fd = field::Empty), ret, err)]
pub fn sock_accept_ready_poll<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ri_waker: WakerId,
    ro_amt: WasmPtr<M::Offset, M>,
) -> Result<Errno, WasiError> {
    // the waker construction needs to be the first line - otherwise errors will leak wakers
    let waker = conv_waker_id(ctx.data().state(), ri_waker);

    wasi_try_ok!(WasiEnv::process_signals_and_wakes_and_exit(&mut ctx)?);

    let env = ctx.data();
    let (memory, state, _) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let amt = wasi_try_ok!(sock_accept_ready_internal::<M>(env, sock, Some(&waker)));

    wasi_try_mem_ok!(ro_amt.write(
        &memory,
        wasi_try_ok!(amt.try_into().map_err(|_| Errno::Overflow))
    ));

    Ok(Errno::Success)
}

pub fn sock_accept_ready_internal<M: MemorySize>(
    env: &WasiEnv,
    sock: WasiFd,
    waker: Option<&Waker>,
) -> Result<usize, Errno> {
    let state = env.state();
    let inodes = &state.inodes;

    let amt = __sock_asyncify(
        env,
        sock,
        Rights::SOCK_ACCEPT,
        waker,
        move |socket, fd| async move { socket.accept_ready().await },
    )?;
    Ok(amt)
}
