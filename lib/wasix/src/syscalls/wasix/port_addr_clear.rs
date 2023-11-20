use super::*;
use crate::syscalls::*;

/// ### `port_addr_clear()`
/// Clears all the addresses on the local port
#[instrument(level = "debug", skip_all, ret)]
pub fn port_addr_clear(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    wasi_try_ok!(port_addr_clear_internal(&mut ctx)?);
    Ok(Errno::Success)
}

pub(crate) fn port_addr_clear_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();
    wasi_try_ok_ok!(__asyncify(ctx, None, async {
        net.ip_clear().await.map_err(net_error_into_wasi_err)
    })?);
    Ok(Ok(()))
}
