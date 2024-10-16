use super::*;
use crate::{net::socket::TimeType, syscalls::*};

/// ### `sock_get_opt_time()`
/// Retrieve one of the times on the socket
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be retrieved
#[instrument(level = "trace", skip_all, fields(%sock, %opt), ret)]
pub fn sock_get_opt_time<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_time: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    let ty = match opt {
        Sockoption::RecvTimeout => TimeType::ReadTimeout,
        Sockoption::SendTimeout => TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => TimeType::AcceptTimeout,
        Sockoption::Linger => TimeType::Linger,
        _ => return Errno::Inval,
    };

    let time = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        |socket, _| socket.opt_time(ty)
    ));

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };

    let time = match time {
        None => OptionTimestamp {
            tag: OptionTag::None,
            u: 0,
        },
        Some(timeout) => OptionTimestamp {
            tag: OptionTag::Some,
            u: timeout.as_nanos() as Timestamp,
        },
    };

    wasi_try_mem!(ret_time.write(&memory, time));

    Errno::Success
}
