use super::*;

impl JournalEffector {
    pub fn save_fd_write<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: u64,
        written: usize,
        iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
        iovs_len: M::Offset,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        let memory = unsafe { env.memory_view(&ctx) };
        let iovs_arr = iovs.slice(&memory, iovs_len)?;

        __asyncify_light(env, None, async {
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
                ctx.data()
                    .runtime()
                    .snapshot_capturer()
                    .write(JournalEntry::FileDescriptorWrite {
                        fd,
                        offset,
                        data: Cow::Borrowed(buf.as_ref()),
                        is_64bit: M::is_64bit(),
                    })
                    .await
                    .map_err(map_snapshot_err)?;
            }
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;
        Ok(())
    }

    pub async fn apply_fd_write<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        offset: u64,
        data: Cow<'_, [u8]>,
    ) -> anyhow::Result<()> {
        let ret = fd_write_internal(
            ctx,
            fd,
            FdWriteSource::<'_, M>::Buffer(data),
            offset,
            None,
            true,
            false,
        )?;
        if ret != Errno::Success {
            bail!(
                "snapshot restore error: failed to write to descriptor (fd={}, offset={}) - {}",
                fd,
                offset,
                ret
            );
        }
        Ok(())
    }
}
