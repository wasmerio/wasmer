use super::*;
use crate::syscalls::*;

/// ### `sock_set_opt_size()
/// Set size of particular option for this socket
/// Note: This is similar to `setsockopt` in POSIX for SO_RCVBUF
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `opt` - Socket option to be set
/// * `size` - Buffer size
pub fn sock_set_opt_size(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    size: Filesize,
) -> Errno {
    debug!(
        "wasi[{}:{}]::sock_set_opt_size(fd={}, ty={})",
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

    let option: crate::state::WasiSocketOption = opt.into();
    wasi_try!(__asyncify(&mut ctx, None, move |ctx| async move {
        __sock_actor_mut(
            ctx,
            sock,
            Rights::empty(),
            move |mut socket| async move {
                match opt {
                    Sockoption::RecvBufSize => socket.set_recv_buf_size(size as usize),
                    Sockoption::SendBufSize => socket.set_send_buf_size(size as usize),
                    Sockoption::Ttl => socket.set_ttl(size as u32),
                    Sockoption::MulticastTtlV4 => socket.set_multicast_ttl_v4(size as u32),
                    _ => Err(Errno::Inval),
                }
            }
        )
        .await
    }));
    Errno::Success
}
