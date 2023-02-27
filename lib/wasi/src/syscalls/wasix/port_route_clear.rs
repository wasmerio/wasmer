use super::*;
use crate::syscalls::*;

/// ### `port_route_clear()`
/// Clears all the routes in the local port
pub fn port_route_clear(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::port_route_clear",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let net = env.net().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async {
        net.route_clear().map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
