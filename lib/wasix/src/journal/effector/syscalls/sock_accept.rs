use std::net::SocketAddr;

use wasmer_wasix_types::wasi::{Addressfamily, SockProto, Socktype};

use crate::{
    fs::Kind,
    net::socket::{InodeSocket, InodeSocketKind, SocketProperties},
};

use super::*;

impl JournalEffector {
    pub fn save_sock_accepted(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        listen_fd: Fd,
        fd: Fd,
        addr: SocketAddr,
        peer_addr: SocketAddr,
        fd_flags: Fdflags,
        nonblocking: bool,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr: addr,
                peer_addr,
                fd_flags,
                non_blocking: nonblocking,
            },
        )
    }

    pub fn apply_sock_accepted(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        _listen_fd: Fd,
        fd: Fd,
        addr: SocketAddr,
        peer_addr: SocketAddr,
        fd_flags: Fdflags,
        nonblocking: bool,
    ) -> anyhow::Result<()> {
        let kind = Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::RemoteSocket {
                local_addr: addr,
                peer_addr,
                ttl: 0,
                multicast_ttl: 0,
                props: SocketProperties {
                    family: match peer_addr.is_ipv4() {
                        true => Addressfamily::Inet4,
                        false => Addressfamily::Inet6,
                    },
                    ty: Socktype::Stream,
                    pt: SockProto::Tcp,
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
            }),
        };

        let env = ctx.data();
        let state = env.state();
        let inodes = &state.inodes;
        let inode = state
            .fs
            .create_inode_with_default_stat(inodes, kind, false, "socket".into());

        let mut new_flags = Fdflags::empty();
        if nonblocking {
            new_flags.set(Fdflags::NONBLOCK, true);
        }

        let mut new_flags = Fdflags::empty();
        if fd_flags.contains(Fdflags::NONBLOCK) {
            new_flags.set(Fdflags::NONBLOCK, true);
        }

        let rights = Rights::all_socket();
        let ret_fd = state
            .fs
            .create_fd(rights, rights, new_flags, 0, inode)
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to create remote accepted socket - {}",
                    err
                )
            })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd, fd);
        if ret != Errno::Success {
            bail!(
                    "journal restore error: failed renumber file descriptor after accepting socket (from={}, to={}) - {}",
                    ret_fd,
                    fd,
                    ret
                );
        }

        Ok(())
    }
}
