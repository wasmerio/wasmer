use super::*;

impl<'a, 'c> JournalSyscallPlayer<'a, 'c> {
    #[allow(clippy::result_large_err)]
    pub(crate) unsafe fn action_fd_write(
        &mut self,
        fd: u32,
        offset: u64,
        data: Cow<'a, [u8]>,
        is_64bit: bool,
    ) -> Result<(), WasiRuntimeError> {
        tracing::trace!(%fd, %offset, "Replay journal - FdWrite");
        if self.stdout_fds.contains(&fd) {
            if let Some(x) = self.stdout.as_mut() {
                x.push(JournalStdIoWrite {
                    offset,
                    data,
                    is_64bit,
                });
            }
            return Ok(());
        }
        if self.stderr_fds.contains(&fd) {
            if let Some(x) = self.stdout.as_mut() {
                x.push(JournalStdIoWrite {
                    offset,
                    data,
                    is_64bit,
                });
            }
            return Ok(());
        }

        if is_64bit {
            JournalEffector::apply_fd_write::<Memory64>(&mut self.ctx, fd, offset, data)
        } else {
            JournalEffector::apply_fd_write::<Memory32>(&mut self.ctx, fd, offset, data)
        }
        .map_err(anyhow_err_to_runtime_err)?;
        Ok(())
    }
}
