use super::*;
use crate::syscalls::*;

/// ### `port_dhcp_acquire()`
/// Acquires a set of IP addresses using DHCP
#[instrument(level = "trace", skip_all, ret)]
pub fn port_dhcp_acquire(mut ctx: FunctionEnvMut<'_, WasiEnv>) -> Result<Errno, WasiError> {
    wasi_try_ok!(port_dhcp_acquire_internal(&mut ctx)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_port_dhcp_acquire(&mut ctx).map_err(|err| {
            tracing::error!("failed to save port_dhcp_acquire event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn port_dhcp_acquire_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();
    let tasks = env.tasks().clone();
    wasi_try_ok_ok!(__asyncify(ctx, None, async move {
        net.dhcp_acquire().await.map_err(net_error_into_wasi_err)
    })?);
    Ok(Ok(()))
}
