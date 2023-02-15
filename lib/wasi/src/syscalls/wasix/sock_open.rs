use super::*;
use crate::syscalls::*;

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
pub fn sock_open<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    af: Addressfamily,
    ty: Socktype,
    pt: SockProto,
    ro_sock: WasmPtr<WasiFd, M>,
) -> Errno {
    debug!("wasi[{}:{}]::sock_open", ctx.data().pid(), ctx.data().tid());

    let env = ctx.data();
    let (memory, state, inodes) = env.get_memory_and_wasi_state_and_inodes(&ctx, 0);

    let kind = match ty {
        Socktype::Stream | Socktype::Dgram => Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::PreSocket {
                family: af,
                ty,
                pt,
                addr: None,
                only_v6: false,
                reuse_port: false,
                reuse_addr: false,
                send_buf_size: None,
                recv_buf_size: None,
                write_timeout: None,
                read_timeout: None,
                accept_timeout: None,
                connect_timeout: None,
            }),
        },
        _ => return Errno::Notsup,
    };

    let inode =
        state
            .fs
            .create_inode_with_default_stat(inodes, kind, false, "socket".to_string().into());
    let rights = Rights::all_socket();
    let fd = wasi_try!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    wasi_try_mem!(ro_sock.write(&memory, fd));

    Errno::Success
}
