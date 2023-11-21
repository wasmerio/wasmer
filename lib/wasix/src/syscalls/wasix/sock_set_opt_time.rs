use super::*;
use crate::{net::socket::TimeType, syscalls::*};

/// ### `sock_set_opt_time()`
/// Sets one of the times the socket
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be set
/// * `time` - Value to set the time to
#[instrument(level = "debug", skip_all, fields(%sock, %opt, time = field::Empty), ret)]
pub fn sock_set_opt_time<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    time: WasmPtr<OptionTimestamp, M>,
) -> Result<Errno, WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let time = wasi_try_mem_ok!(time.read(&memory));
    let time = match time.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(time.u)),
        _ => return Ok(Errno::Inval),
    };
    Span::current().record("time", &format!("{:?}", time));

    let ty = match opt {
        Sockoption::RecvTimeout => TimeType::ReadTimeout,
        Sockoption::SendTimeout => TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => TimeType::AcceptTimeout,
        Sockoption::Linger => TimeType::Linger,
        _ => return Ok(Errno::Inval),
    };

    wasi_try_ok!(sock_set_opt_time_internal(&mut ctx, sock, ty, time)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_set_opt_time(&mut ctx, sock, ty, time).map_err(|err| {
            tracing::error!("failed to save sock_set_opt_time event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn sock_set_opt_time_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    ty: TimeType,
    time: Option<Duration>,
) -> Result<Result<(), Errno>, WasiError> {
    wasi_try_ok_ok!(__sock_actor_mut(ctx, sock, Rights::empty(), |socket, _| {
        socket.set_opt_time(ty, time)
    }));

    Ok(Ok(()))
}
