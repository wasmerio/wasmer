use virtual_net::IpCidr;

use super::*;
use crate::syscalls::*;

/// ### `port_addr_add()`
/// Adds another static address to the local port
///
/// ## Parameters
///
/// * `addr` - Address to be added
#[instrument(level = "debug", skip_all, fields(ip = field::Empty), ret)]
pub fn port_addr_add<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    ip: WasmPtr<__wasi_cidr_t, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let cidr = wasi_try_ok!(crate::net::read_cidr(&memory, ip));
    Span::current().record("ip", &format!("{:?}", cidr));

    wasi_try_ok!(port_addr_add_internal(&mut ctx, cidr)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_port_addr_add(&mut ctx, cidr).map_err(|err| {
            tracing::error!("failed to save port_addr_add event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn port_addr_add_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    cidr: IpCidr,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();
    wasi_try_ok_ok!(__asyncify(ctx, None, async {
        net.ip_add(cidr.ip, cidr.prefix)
            .await
            .map_err(net_error_into_wasi_err)
    })?);
    Ok(Ok(()))
}
