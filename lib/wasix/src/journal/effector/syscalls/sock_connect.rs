use std::net::SocketAddr;

use wasmer_wasix_types::wasi::{Addressfamily, SockProto, Socktype};

use crate::{
    fs::Kind,
    net::socket::{InodeSocket, InodeSocketKind, SocketProperties},
};

use super::*;

impl JournalEffector {
    pub fn save_sock_connect(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            },
        )
    }

    pub fn apply_sock_connect(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        local_addr: SocketAddr,
        peer_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        let kind = Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::RemoteSocket {
                local_addr,
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

        let rights = Rights::all_socket();
        let ret_fd = state
            .fs
            .create_fd(rights, rights, Fdflags::empty(), 0, inode)
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to create remote connected socket - {}",
                    err
                )
            })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd, fd);
        if ret != Errno::Success {
            bail!(
                    "journal restore error: failed renumber file descriptor after connecting the socket (from={}, to={}) - {}",
                    ret_fd,
                    fd,
                    ret
                );
        }

        Ok(())
    }
}
