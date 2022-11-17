use super::*;
use crate::syscalls::*;

/// Causes a fault on a particular call that was made
/// to this process from another process; where 'bid'
/// is the callering process context.
///
/// ## Parameters
///
/// * `cid` - Handle of the call to raise a fault on
/// * `fault` - Fault to be raised on the bus
pub fn call_fault(ctx: FunctionEnvMut<'_, WasiEnv>, cid: Cid, fault: BusErrno) {
    let env = ctx.data();
    let bus = env.runtime.bus();
    debug!(
        "wasi[{}:{}]::call_fault (cid={}, fault={})",
        ctx.data().pid(),
        ctx.data().tid(),
        cid,
        fault
    );

    let mut guard = env.state.bus.protected();
    guard.calls.remove(&cid);

    if let Some(call) = guard.called.remove(&cid) {
        drop(guard);
        call.fault(bus_errno_into_vbus_error(fault));
    }
}
