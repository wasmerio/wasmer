use super::*;
use crate::syscalls::*;

/// Closes a bus process and releases all associated resources
///
/// ## Parameters
///
/// * `bid` - Handle of the bus process handle to be closed
pub fn bus_close(_ctx: FunctionEnvMut<'_, WasiEnv>, _bid: Bid) -> BusErrno {
    BusErrno::Unsupported
}
