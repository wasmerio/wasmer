use wasmer_wasix_types::wasi::EventFdFlags;

use super::*;

impl JournalEffector {
    pub fn save_fd_event(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        initial_val: u64,
        flags: EventFdFlags,
        fd: Fd,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::CreateEventV1 {
                initial_val,
                flags,
                fd,
            },
        )
    }

    pub fn apply_fd_event(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        initial_val: u64,
        flags: EventFdFlags,
        fd: Fd,
    ) -> anyhow::Result<()> {
        let ret_fd = crate::syscalls::fd_event_internal(ctx, initial_val, flags)
            .map(|r| r.map_err(|err| err.to_string()))
            .unwrap_or_else(|err| Err(err.to_string()))
            .map_err(|err| {
                anyhow::format_err!("journal restore error: failed to create event - {}", err)
            })?;

        let ret = crate::syscalls::fd_renumber_internal(ctx, ret_fd, fd);
        if ret != Errno::Success {
            bail!(
                "journal restore error: failed renumber file descriptor after create event (from={}, to={}) - {}",
                ret_fd,
                fd,
                ret
            );
        }

        Ok(())
    }
}
