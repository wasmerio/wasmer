use super::*;
use crate::syscalls::*;

/// ### `sock_get_opt_size()`
/// Retrieve the size of particular option for this socket
/// Note: This is similar to `getsockopt` in POSIX for SO_RCVBUF
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `sockopt` - Socket option to be retrieved
#[instrument(level = "debug", skip_all, fields(%sock, %opt), ret)]
pub fn sock_get_opt_size<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    ret_size: WasmPtr<Filesize, M>,
) -> Errno {
    let size = wasi_try!(__sock_actor(
        &mut ctx,
        sock,
        Rights::empty(),
        |socket, _| match opt {
            Sockoption::RecvBufSize => socket.recv_buf_size().map(|a| a as Filesize),
            Sockoption::SendBufSize => socket.send_buf_size().map(|a| a as Filesize),
            Sockoption::Ttl => socket.ttl().map(|a| a as Filesize),
            Sockoption::MulticastTtlV4 => {
                socket.multicast_ttl_v4().map(|a| a as Filesize)
            }
            _ => Err(Errno::Inval),
        }
    ));

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    wasi_try_mem!(ret_size.write(&memory, size));

    Errno::Success
}
