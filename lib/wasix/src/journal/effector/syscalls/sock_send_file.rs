use crate::syscalls::sock_send_file_internal;

use super::*;

impl JournalEffector {
    pub fn save_sock_send_file<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        socket_fd: Fd,
        file_fd: Fd,
        offset: Filesize,
        count: Filesize,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            },
        )
    }

    pub fn apply_sock_send_file(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        socket_fd: Fd,
        file_fd: Fd,
        offset: Filesize,
        count: Filesize,
    ) -> anyhow::Result<()> {
        sock_send_file_internal(ctx, socket_fd, file_fd, offset, count)?.map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to send_file on socket (sock={}, in_fd={}, offset={}, count={}) - {}",
                socket_fd,
                file_fd,
                offset,
                count,
                err
            )
        })?;
        Ok(())
    }
}
