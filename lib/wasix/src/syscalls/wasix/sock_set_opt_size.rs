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
#[instrument(level = "debug", skip_all, fields(%sock, %opt, %size), ret)]
pub fn sock_set_opt_size(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    size: Filesize,
) -> Result<Errno, WasiError> {
    wasi_try_ok!(sock_set_opt_size_internal(&mut ctx, sock, opt, size)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_set_opt_size(&mut ctx, sock, opt, size).map_err(|err| {
            tracing::error!("failed to save sock_set_opt_size event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}

pub(crate) fn sock_set_opt_size_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    opt: Sockoption,
    size: Filesize,
) -> Result<Result<(), Errno>, WasiError> {
    let ty = match opt {
        Sockoption::RecvTimeout => TimeType::ReadTimeout,
        Sockoption::SendTimeout => TimeType::WriteTimeout,
        Sockoption::ConnectTimeout => TimeType::ConnectTimeout,
        Sockoption::AcceptTimeout => TimeType::AcceptTimeout,
        Sockoption::Linger => TimeType::Linger,
        _ => return Ok(Err(Errno::Inval)),
    };

    let option: crate::net::socket::WasiSocketOption = opt.into();
    wasi_try_ok_ok!(__sock_actor_mut(
        ctx,
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
    Ok(Ok(()))
}
