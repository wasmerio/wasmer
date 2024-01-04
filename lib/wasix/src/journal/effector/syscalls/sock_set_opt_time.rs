use std::time::Duration;

use crate::net::socket::TimeType;

use super::*;

impl JournalEffector {
    pub fn save_sock_set_opt_time(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        ty: TimeType,
        time: Option<Duration>,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketSetOptTimeV1 {
                fd,
                ty: ty.into(),
                time,
            },
        )
    }

    pub fn apply_sock_set_opt_time(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        ty: TimeType,
        time: Option<Duration>,
    ) -> anyhow::Result<()> {
        crate::syscalls::sock_set_opt_time_internal(ctx, fd, ty, time)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to set socket option (fd={}, opt={:?}, time={:?}) - {}",
                    fd,
                    ty,
                    time,
                    err
                )
            })?;
        Ok(())
    }
}
