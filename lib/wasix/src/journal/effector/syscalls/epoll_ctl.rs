use super::*;

impl JournalEffector {
    pub fn save_epoll_ctl(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        epfd: Fd,
        op: EpollCtl,
        fd: Fd,
        event: Option<EpollEventCtl>,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            },
        )
    }

    pub fn apply_epoll_ctl(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        pfd: Fd,
        op: EpollCtl,
        fd: Fd,
        event: Option<EpollEventCtl>,
    ) -> anyhow::Result<()> {
        crate::syscalls::epoll_ctl_internal(ctx, pfd, op, fd, event.as_ref())
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to epoll ctl (pfd={}, op={:?}, fd={}) - {}",
                    pfd,
                    op,
                    fd,
                    err
                )
            })?
            .map_err(|err| {
                anyhow::format_err!(
                    "journal restore error: failed to epoll ctl (pfd={}, op={:?}, fd={}) - {}",
                    pfd,
                    op,
                    fd,
                    err
                )
            })?;

        Ok(())
    }
}
