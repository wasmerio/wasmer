use wasmer_wasix_types::wasi::Sockoption;

use super::*;

impl JournalEffector {
    pub fn save_sock_set_opt_size(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        opt: Sockoption,
        size: Filesize,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::SocketSetOptSizeV1 { fd, opt, size })
    }

    pub fn apply_sock_set_opt_size(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        opt: Sockoption,
        size: Filesize,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_set_opt_size_internal(ctx, fd, opt, size)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to set socket option (fd={}, opt={:?}, size={}) - {}",
                    fd,
                    opt,
                    size,
                    err
                )
            })?;
        Ok(())
    }
}
