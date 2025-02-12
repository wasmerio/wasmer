use super::*;

impl JournalEffector {
    pub fn save_fd_write<M: MemorySize>(
        ctx: &FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        mut offset: u64,
        written: usize,
        iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
        iovs_len: M::Offset,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        let memory = unsafe { env.memory_view(&ctx) };
        let iovs_arr = iovs.slice(&memory, iovs_len)?;

        let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
        let mut remaining: M::Offset = TryFrom::<usize>::try_from(written).unwrap_or_default();
        for iovs in iovs_arr.iter() {
            let sub = iovs.buf_len.min(remaining);
            if sub == M::ZERO {
                continue;
            }
            remaining -= sub;

            let buf = WasmPtr::<u8, M>::new(iovs.buf)
                .slice(&memory, sub)
                .map_err(mem_error_to_wasi)?
                .access()
                .map_err(mem_error_to_wasi)?;
            let data = Cow::Borrowed(buf.as_ref());
            let data_len = data.len();

            ctx.data()
                .active_journal()?
                .write(JournalEntry::FileDescriptorWriteV1 {
                    fd,
                    offset,
                    data,
                    is_64bit: M::is_64bit(),
                })
                .map_err(map_snapshot_err)?;

            offset += data_len as u64;
        }
        Ok(())
    }

    pub fn apply_fd_write<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: u64,
        data: Cow<'_, [u8]>,
    ) -> anyhow::Result<()> {
        fd_write_internal(
            ctx,
            fd,
            FdWriteSource::<'_, M>::Buffer(data),
            offset,
            true,
            false,
        )?
        .map_err(|err| {
            anyhow::format_err!(
                "journal restore error: failed to write to descriptor (fd={}, offset={}) - {}",
                fd,
                offset,
                err
            )
        })?;
        Ok(())
    }
}
