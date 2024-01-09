use virtual_net::IpCidr;

use super::*;
use crate::syscalls::*;

/// ### `port_route_add()`
/// Adds a new route to the local port
#[instrument(level = "debug", skip_all, fields(cidr = field::Empty, via_router = field::Empty), ret)]
pub fn port_route_add<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    cidr: WasmPtr<__wasi_cidr_t, M>,
    via_router: WasmPtr<__wasi_addr_t, M>,
    preferred_until: WasmPtr<OptionTimestamp, M>,
    expires_at: WasmPtr<OptionTimestamp, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let cidr = wasi_try_ok!(crate::net::read_cidr(&memory, cidr));
    Span::current().record("cidr", &format!("{:?}", cidr));

    let via_router = wasi_try_ok!(crate::net::read_ip(&memory, via_router));
    Span::current().record("via_router", &format!("{:?}", via_router));

    let preferred_until = wasi_try_mem_ok!(preferred_until.read(&memory));
    let preferred_until = match preferred_until.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(preferred_until.u)),
        _ => return Ok(Errno::Inval),
    };
    let expires_at = wasi_try_mem_ok!(expires_at.read(&memory));
    let expires_at = match expires_at.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(expires_at.u)),
        _ => return Ok(Errno::Inval),
    };

    wasi_try_ok!(port_route_add_internal(
        &mut ctx,
        cidr,
        via_router,
        preferred_until,
        expires_at
    )?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_port_route_add(
            &mut ctx,
            cidr,
            via_router,
            preferred_until,
            expires_at,
        )
        .map_err(|err| {
            tracing::error!("failed to save port_route_add event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn port_route_add_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    cidr: IpCidr,
    via_router: IpAddr,
    preferred_until: Option<Duration>,
    expires_at: Option<Duration>,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();
    wasi_try_ok_ok!(__asyncify(ctx, None, async {
        net.route_add(cidr, via_router, preferred_until, expires_at)
            .await
            .map_err(net_error_into_wasi_err)
    })?);

    Ok(Ok(()))
}
