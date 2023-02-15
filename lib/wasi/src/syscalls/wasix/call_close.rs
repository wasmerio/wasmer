use super::*;
use crate::syscalls::*;

// FIXME: remove , since it's no longer used.
/// Closes a bus call based on its bus call handle
///
/// ## Parameters
///
/// * `cid` - Handle of the bus call handle to be dropped
pub fn call_close(_ctx: FunctionEnvMut<'_, WasiEnv>, _cid: Cid) {}
