use virtual_net::SocketAddr;

use crate::{
    fs::Kind,
    net::socket::{InodeSocket, InodeSocketKind},
};

use super::*;

impl JournalEffector {
    pub fn save_sock_accepted(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        listen_fd: Fd,
        fd: Fd,
        peer_addr: SocketAddr,
        fd_flags: Fdflags,
        nonblocking: bool,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketAccepted {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                nonblocking,
            },
        )
    }

    pub fn apply_sock_accepted(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        _listen_fd: Fd,
        fd: Fd,
        peer_addr: SocketAddr,
        fd_flags: Fdflags,
        nonblocking: bool,
    ) -> anyhow::Result<()> {
        let kind = Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::RemoteTcpStream { peer_addr }),
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
