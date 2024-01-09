use super::*;
use crate::syscalls::*;

/// ### `port_gateway_set()`
/// Adds a default gateway to the port
///
/// ## Parameters
///
/// * `addr` - Address of the default gateway
#[instrument(level = "debug", skip_all, fields(ip = field::Empty), ret)]
pub fn port_gateway_set<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_addr_t, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let ip = wasi_try_ok!(crate::net::read_ip(&memory, ip));
    Span::current().record("ip", &format!("{:?}", ip));

    wasi_try_ok!(port_gateway_set_internal(&mut ctx, ip)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_port_gateway_set(&mut ctx, ip).map_err(|err| {
            tracing::error!("failed to save port_gateway_set event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn port_gateway_set_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    ip: IpAddr,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();
    wasi_try_ok_ok!(__asyncify(ctx, None, async {
        net.gateway_set(ip).await.map_err(net_error_into_wasi_err)
    })?);
    Ok(Ok(()))
}
