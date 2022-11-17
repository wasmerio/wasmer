use super::*;
use crate::syscalls::*;

/// Closes a bus process and releases all associated resources
///
/// ## Parameters
///
/// * `bid` - Handle of the bus process handle to be closed
pub fn bus_close(ctx: FunctionEnvMut<'_, WasiEnv>, bid: Bid) -> BusErrno {
    trace!(
        "wasi[{}:{}]::bus_close (bid={})",
        ctx.data().pid(),
        ctx.data().tid(),
        bid
    );
    let pid: WasiProcessId = bid.into();

    let env = ctx.data();
    let mut inner = env.process.write();
    if let Some(process) = inner.bus_processes.remove(&pid) {
        inner.bus_process_reuse.retain(|_, v| *v != pid);
    }

    BusErrno::Success
}