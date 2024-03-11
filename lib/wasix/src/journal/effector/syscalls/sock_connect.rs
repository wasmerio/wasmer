use std::net::SocketAddr;

use crate::{
    fs::Kind,
    net::socket::{InodeSocket, InodeSocketKind},
};

use super::*;

impl JournalEffector {
    pub fn save_sock_connect(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::SocketConnectedV1 { fd, addr })
    }

    pub fn apply_sock_connect(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        addr: SocketAddr,
    ) -> anyhow::Result<()> {
        let kind = Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::RemoteTcpStream { peer_addr: addr }),
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
