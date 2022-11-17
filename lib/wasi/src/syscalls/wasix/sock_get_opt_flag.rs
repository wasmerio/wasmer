use super::*;
use crate::syscalls::*;

/// ### `sock_get_opt_flag()`
/// Retrieve status of particular socket seting
/// Note: This is similar to `getsockopt` in POSIX for SO_REUSEADDR
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be retrieved
pub fn sock_get_opt_flag<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_flag: WasmPtr<Bool, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_get_opt_flag(fd={}, ty={})",
        ctx.data().pid(),
        ctx.data().tid(),
        sock,
        opt
    );

    let option: crate::state::WasiSocketOption = opt.into();
    let flag = wasi_try!(__asyncify(&mut ctx, None, async move {
        __sock_actor(
            &mut ctx,
            sock,
            Rights::empty(),
            move |socket| async move { socket.get_opt_flag(option) }
        )
        .await
    }));

    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let flag = match flag {
        false => Bool::False,
        true => Bool::True,
    };

    wasi_try_mem!(ret_flag.write(&memory, flag));

    Errno::Success
}
