use super::*;

impl JournalEffector {
    pub fn save_epoll_create(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::EpollCreateV1 { fd })
    }

    pub fn apply_epoll_create(ctx: &mut FunctionEnvMut<'_, WasiEnv>, fd: Fd) -> anyhow::Result<()> {
        let ret_fd = crate::syscalls::epoll_create_internal(ctx, Some(fd))
            .map_err(|err| {
                anyhow::format_err!("journal restore error: failed to create epoll - {}", err)
            })?
            .map_err(|err| {
                anyhow::format_err!("journal restore error: failed to create epoll - {}", err)
            })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd, fd);
        if !matches!(ret, Ok(Errno::Success)) {
            bail!(
                "journal restore error: failed renumber file descriptor after epoll create (from={}, to={}) - {}",
                ret_fd,
                fd,
                ret.unwrap_or(Errno::Unknown)
            );
        }

        Ok(())
    }
}
