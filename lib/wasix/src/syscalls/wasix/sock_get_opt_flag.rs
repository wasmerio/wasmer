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
#[instrument(level = "debug", skip_all, fields(%sock, %opt), ret)]
pub fn sock_get_opt_flag<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_flag: WasmPtr<Bool, M>,
) -> Errno {
    let option: crate::net::socket::WasiSocketOption = opt.into();
    let flag = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        |socket, _| socket.get_opt_flag(option)
    ));

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let flag = match flag {
        false => Bool::False,
        true => Bool::True,
    };

    wasi_try_mem!(ret_flag.write(&memory, flag));

    Errno::Success
}
