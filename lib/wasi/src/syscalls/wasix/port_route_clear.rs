use super::*;
use crate::syscalls::*;

/// ### `port_route_clear()`
/// Clears all the routes in the local port
pub fn port_route_clear(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!(
        "wasi[{}:{}]::port_route_clear",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    wasi_try!(env.net().route_clear().map_err(net_error_into_wasi_err));
    Errno::Success
}