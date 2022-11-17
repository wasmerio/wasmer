use super::*;
use crate::syscalls::*;

/// ### `port_unbridge()`
/// Disconnects from a remote network
pub fn port_unbridge(ctx: FunctionEnvMut<'_, WasiEnv>) -> Errno {
    debug!(
        "wasi[{}:{}]::port_unbridge",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let env = ctx.data();
    wasi_try!(env.net().unbridge().map_err(net_error_into_wasi_err));
    Errno::Success
}
