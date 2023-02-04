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
pub fn sock_set_opt_time<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    time: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_set_opt_time(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let time = wasi_try_mem!(time.read(&memory));
    let time = match time.tag {
        OptionTag::None => None,
        OptionTag::Some => Some(Duration::from_nanos(time.u)),
        _ => return Errno::Inval,
    };

    let ty = match opt {
        Sockoption::RecvTimeout => TimeType::ReadTimeout,
        Sockoption::SendTimeout => TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => TimeType::AcceptTimeout,
        Sockoption::Linger => TimeType::Linger,
        _ => return Errno::Inval,
    };

    let option: crate::net::socket::WasiSocketOption = opt.into();
    wasi_try!(__sock_actor_mut(
        &mut ctx,
        sock,
        Rights::empty(),
        |socket, _| socket.set_opt_time(ty, time)
    ));
    Errno::Success
}
