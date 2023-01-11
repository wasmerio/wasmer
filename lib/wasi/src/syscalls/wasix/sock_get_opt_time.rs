use super::*;
use crate::syscalls::*;

/// ### `sock_get_opt_time()`
/// Retrieve one of the times on the socket
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be retrieved
pub fn sock_get_opt_time<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_time: WasmPtr<OptionTimestamp, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_get_opt_time(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );

    let ty = match opt {
        Sockoption::RecvTimeout => wasmer_vnet::TimeType::ReadTimeout,
        Sockoption::SendTimeout => wasmer_vnet::TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => wasmer_vnet::TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => wasmer_vnet::TimeType::AcceptTimeout,
        Sockoption::Linger => wasmer_vnet::TimeType::Linger,
        _ => return Errno::Inval,
    };

    let time = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        move |socket| async move { socket.opt_time(ty).await }
    ));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);

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
