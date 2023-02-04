use super::*;
use crate::{net::socket::TimeType, syscalls::*};

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
        |mut socket, _| match opt {
            Sockoption::RecvBufSize => socket.set_recv_buf_size(size as usize),
            Sockoption::SendBufSize => socket.set_send_buf_size(size as usize),
            Sockoption::Ttl => socket.set_ttl(size as u32),
            Sockoption::MulticastTtlV4 => socket.set_multicast_ttl_v4(size as u32),
            _ => Err(Errno::Inval),
        }
    ));
    Errno::Success
}
