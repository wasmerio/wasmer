use std::task::Waker;

use wasmer_wasix_types::wasi::WakerId;

use super::*;
use crate::{state::conv_waker_id, syscalls::*};

/// ### `thread_join_poll()`
///
/// Polls to joins this thread with another thread, blocking this
/// one until the other finishes. If the thread can not join now
/// then a waker will be registered for when the thread has joined
///
/// ## Parameters
///
/// * `tid` - Handle of the thread to wait on
/// * `waker` - Waker ID that will be passed back to the program when the waker is triggered
///
//#[instrument(level = "debug", skip_all, fields(%join_tid), ret, err)]
pub fn thread_join_poll<M: MemorySize + 'static>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    join_tid: Tid,
    waker: WakerId,
) -> Result<Errno, WasiError> {
    let waker = conv_waker_id(ctx.data().state(), waker);
    thread_join_internal::<M>(ctx, join_tid, Some(&waker))
}
