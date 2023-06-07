use super::*;
use crate::syscalls::*;

/// ### `port_unbridge()`
/// Disconnects from a remote network
#[instrument(level = "debug", skip_all, ret, err)]
pub fn port_unbridge(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();
    wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        net.unbridge().await.map_err(net_error_into_wasi_err)
    })?);
    Ok(Errno::Success)
}
