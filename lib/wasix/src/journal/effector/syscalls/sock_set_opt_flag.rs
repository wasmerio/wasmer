use wasmer_wasix_types::wasi::Sockoption;

use super::*;

impl JournalEffector {
    pub fn save_sock_set_opt_flag(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        opt: Sockoption,
        flag: bool,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::SocketSetOptFlagV1 { fd, opt, flag })
    }

    pub fn apply_sock_set_opt_flag(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        opt: Sockoption,
        flag: bool,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_set_opt_flag_internal(ctx, fd, opt, flag)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to set socket option (fd={}, opt={:?}, flag={}) - {}",
                    fd,
                    opt,
                    flag,
                    err
                )
            })?;
        Ok(())
    }
}
