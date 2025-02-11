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
        dead: bool,
    ) -> anyhow::Result<()> {
        let kind = Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::RemoteSocket {
                is_dead: dead,
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
        state
            .fs
            .with_fd(
                rights,
                rights,
                Fdflags::empty(),
                Fdflagsext::empty(),
                0,
                inode,
                fd,
            )
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to create remote connected socket - {}",
                    err
                )
            })?;

        Ok(())
    }
}
