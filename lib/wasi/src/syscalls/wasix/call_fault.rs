use super::*;
use crate::syscalls::*;

// FIXME: remove , since it's no longer used.
/// Causes a fault on a particular call that was made
/// to this process from another process; where 'bid'
/// is the callering process context.
///
/// ## Parameters
///
/// * `cid` - Handle of the call to raise a fault on
/// * `fault` - Fault to be raised on the bus
pub fn call_fault(_ctx: FunctionEnvMut<'_, WasiEnv>, _cid: Cid, _fault: BusErrno) {}
