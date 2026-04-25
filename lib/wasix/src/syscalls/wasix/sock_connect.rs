use super::*;
use crate::net::socket::TimeType;
use crate::syscalls::*;

/// ### `sock_connect()`
/// Initiate a connection on a socket to the specified address
///
/// Polling the socket handle will wait for data to arrive or for
/// the socket status to change which can be queried via 'sock_status'
///
/// Note: This is similar to `connect` in POSIX
///
/// ## Parameters
///
/// * `fd` - Socket descriptor
/// * `addr` - Address of the socket to connect to
#[instrument(level = "trace", skip_all, fields(%sock, addr = field::Empty), ret)]
pub fn sock_connect<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: WasmPtr<__wasi_addr_port_t, M>,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let addr = wasi_try_ok!(crate::net::read_ip_port(&memory, addr));
    let peer_addr = SocketAddr::new(addr.0, addr.1);
    Span::current().record("addr", format!("{peer_addr:?}"));

    wasi_try_ok!(sock_connect_internal(&mut ctx, sock, peer_addr)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        let local_addr = wasi_try_ok!(__sock_actor(
            &mut ctx,
            sock,
            Rights::empty(),
            |socket, _| socket.addr_local()
        ));
        JournalEffector::save_sock_connect(&mut ctx, sock, local_addr, peer_addr).map_err(
            |err| {
                tracing::error!("failed to save sock_connected event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            },
        )?;
    }

    Ok(Errno::Success)
}

fn nonblocking_connect_result(status: crate::net::socket::WasiSocketStatus) -> Result<(), Errno> {
    match status {
        crate::net::socket::WasiSocketStatus::Opening => Err(Errno::Inprogress),
        crate::net::socket::WasiSocketStatus::Opened => Ok(()),
        crate::net::socket::WasiSocketStatus::Closed
        | crate::net::socket::WasiSocketStatus::Failed => Err(Errno::Notconn),
    }
}

pub(crate) fn sock_connect_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    sock: WasiFd,
    addr: SocketAddr,
) -> Result<Result<(), Errno>, WasiError> {
    let env = ctx.data();
    let net = env.net().clone();
    let tasks = ctx.data().tasks().clone();
    let nonblocking = match env.state.fs.get_fd(sock) {
        Ok(fd_entry) => fd_entry.inner.flags.contains(Fdflags::NONBLOCK),
        Err(err) => return Ok(Err(err)),
    };
    wasi_try_ok_ok!(__sock_upgrade(
        ctx,
        sock,
        Rights::SOCK_CONNECT,
        move |mut socket, flags| async move {
            // Auto-bind UDP
            socket = socket
                .auto_bind_udp(tasks.deref(), net.deref())
                .await?
                .unwrap_or(socket);
            let timeout = socket.opt_time(TimeType::ConnectTimeout).ok().flatten();
            socket
                .connect(
                    tasks.deref(),
                    net.deref(),
                    addr,
                    timeout,
                    flags.contains(Fdflags::NONBLOCK),
                )
                .await
        }
    ));

    if nonblocking {
        let status = match __sock_actor(ctx, sock, Rights::empty(), |socket, _| socket.status()) {
            Ok(status) => status,
            Err(err) => return Ok(Err(err)),
        };
        return Ok(nonblocking_connect_result(status));
    }

    Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::nonblocking_connect_result;
    use crate::net::socket::WasiSocketStatus;
    use wasmer_wasix_types::wasi::Errno;

    #[test]
    fn nonblocking_connect_result_maps_socket_states() {
        assert_eq!(
            nonblocking_connect_result(WasiSocketStatus::Opening),
            Err(Errno::Inprogress)
        );
        assert_eq!(nonblocking_connect_result(WasiSocketStatus::Opened), Ok(()));
        assert_eq!(
            nonblocking_connect_result(WasiSocketStatus::Failed),
            Err(Errno::Notconn)
        );
        assert_eq!(
            nonblocking_connect_result(WasiSocketStatus::Closed),
            Err(Errno::Notconn)
        );
    }
}
