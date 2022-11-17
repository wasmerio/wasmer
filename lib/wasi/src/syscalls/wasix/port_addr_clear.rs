use super::*;
use crate::syscalls::*;

/// ### `port_addr_clear()`
/// Clears all the addresses on the local port
pub fn port_addr_clear(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!(
        "wasi[{}:{}]::port_addr_clear",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    wasi_try!(env.net().ip_clear().map_err(net_error_into_wasi_err));
    Errno::Success
}