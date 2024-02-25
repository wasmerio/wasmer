use super::*;
use crate::{net::socket::SocketProperties, syscalls::*};

/// ### `sock_open()`
/// Create an endpoint for communication.
///
/// creates an endpoint for communication and returns a file descriptor
/// tor that refers to that endpoint. The file descriptor returned by a successful
/// call will be the lowest-numbered file descriptor not currently open
/// for the process.
///
/// Note: This is similar to `socket` in POSIX using PF_INET
///
/// ## Parameters
///
/// * `af` - Address family
/// * `socktype` - Socket type, either datagram or stream
/// * `sock_proto` - Socket protocol
///
/// ## Return
///
/// The file descriptor of the socket that has been opened.
#[instrument(level = "debug", skip_all, fields(?af, ?ty, ?pt, sock = field::Empty), ret)]
pub fn sock_open<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    af: Addressfamily,
    ty: Socktype,
    pt: SockProto,
    ro_sock: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    // only certain combinations are supported
    match pt {
        SockProto::Tcp => {
            if ty != Socktype::Stream {
                return Ok(Errno::Notsup);
            }
        }
        SockProto::Udp => {
            if ty != Socktype::Dgram {
                return Ok(Errno::Notsup);
            }
        }
        _ => {}
    }

    let fd = wasi_try_ok!(sock_open_internal(&mut ctx, af, ty, pt)?);

    #[cfg(feature = "journal")]
    if ctx.data().enable_journal {
        JournalEffector::save_sock_open(&mut ctx, af, ty, pt, fd).map_err(|err| {
            tracing::error!("failed to save sock_open event - {}", err);
            WasiError::Exit(ExitCode::Errno(Errno::Fault))
        })?;
    }

    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };
    wasi_try_mem_ok!(ro_sock.write(&memory, fd));

    Ok(Errno::Success)
}

pub(crate) fn sock_open_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    af: Addressfamily,
    ty: Socktype,
    pt: SockProto,
) -> Result<Result<WasiFd, Errno>, WasiError> {
    let env = ctx.data();
    let (memory, state, inodes) = unsafe { env.get_memory_and_wasi_state_and_inodes(&ctx, 0) };

    let kind = match ty {
        Socktype::Stream | Socktype::Dgram => Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::PreSocket {
                props: SocketProperties {
                    family: af,
                    ty,
                    pt,
                    only_v6: false,
                    reuse_port: false,
                    reuse_addr: false,
                    no_delay: None,
                    keep_alive: None,
                    dont_route: None,
                    send_buf_size: None,
                    recv_buf_size: None,
                    write_timeout: None,
                    read_timeout: None,
                    accept_timeout: None,
                    connect_timeout: None,
                    handler: None,
                },
                addr: None,
            }),
        },
        _ => return Ok(Err(Errno::Notsup)),
    };

    let inode =
        state
            .fs
            .create_inode_with_default_stat(inodes, kind, false, "socket".to_string().into());
    let rights = Rights::all_socket();
    let fd = wasi_try_ok_ok!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));
    Span::current().record("sock", fd);

    Ok(Ok(fd))
}
