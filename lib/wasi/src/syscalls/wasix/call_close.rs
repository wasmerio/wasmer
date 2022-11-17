use super::*;
use crate::syscalls::*;

/// Closes a bus call based on its bus call handle
///
/// ## Parameters
///
/// * `cid` - Handle of the bus call handle to be dropped
pub fn call_close(ctx: FunctionEnvMut<'_, WasiEnv>, cid: Cid) {
    let env = ctx.data();
    let bus = env.runtime.bus();
    trace!(
        "wasi[{}:{}]::call_close (cid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        cid
    );

    let mut guard = env.state.bus.protected();
    guard.calls.remove(&cid);
    guard.called.remove(&cid);
}
