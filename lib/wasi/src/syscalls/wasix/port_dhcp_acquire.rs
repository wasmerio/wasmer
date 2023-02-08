use super::*;
use crate::syscalls::*;

/// ### `port_dhcp_acquire()`
/// Acquires a set of IP addresses using DHCP
pub fn port_dhcp_acquire(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::port_dhcp_acquire",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    let net = env.net().clone();
    let tasks = env.tasks().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        net.dhcp_acquire().await.map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
