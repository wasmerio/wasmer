use wasmer_wasix_types::wasi::{Addressfamily, SockProto, Socktype};

use super::*;

impl JournalEffector {
    pub fn save_sock_open(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        af: Addressfamily,
        ty: Socktype,
        pt: SockProto,
        fd: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::SocketOpenV1 { af, ty, pt, fd })
    }

    pub fn apply_sock_open(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        af: Addressfamily,
        ty: Socktype,
        pt: SockProto,
        fd: Fd,
    ) -> anyhow::Result<()> {
        let ret_fd = crate::syscalls::sock_open_internal(ctx, af, ty, pt)
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to open socket (af={:?}, ty={:?}, pt={:?}) - {}",
                    af,
                    ty,
                    pt,
                    err
                )
            })?
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to open socket (af={:?}, ty={:?}, pt={:?}) - {}",
                    af,
                    ty,
                    pt,
                    err
                )
            })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd, fd);
        if ret != Errno::Success {
            bail!(
                    "journal restore error: failed renumber file descriptor after opening socket (from={}, to={}) - {}",
                    ret_fd,
                    fd,
                    ret
                );
        }
        Ok(())
    }
}
