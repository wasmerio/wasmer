use wasmer_wasix_types::wasi::SiFlags;

use crate::syscalls::sock_send_internal;

use super::*;

impl JournalEffector {
    pub fn save_sock_send<M: MemorySize>(
        ctx: &FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        sent: usize,
        iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
        iovs_len: M::Offset,
        si_flags: SiFlags,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        let memory = unsafe { env.memory_view(&ctx) };
        let iovs_arr = iovs.slice(&memory, iovs_len)?;

        let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
        let mut remaining: M::Offset = TryFrom::<usize>::try_from(sent).unwrap_or_default();
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
                .active_journal()?
                .write(JournalEntry::SocketSendV1 {
                    fd,
                    data: Cow::Borrowed(buf.as_ref()),
                    is_64bit: M::is_64bit(),
                    flags: si_flags,
                })
                .map_err(map_snapshot_err)?;
        }
        Ok(())
    }

    pub fn apply_sock_send<M: MemorySize>(
        ctx: &FunctionEnvMut<'_, WasiEnv>,
        sock: Fd,
        si_data: Cow<'_, [u8]>,
        si_flags: SiFlags,
    ) -> anyhow::Result<()> {
        let data_len = si_data.len();
        sock_send_internal(ctx, sock, FdWriteSource::<'_, M>::Buffer(si_data), si_flags)?.map_err(
            |err| {
                anyhow::format_err!(
                    "journal restore error: failed to send on socket (fd={}, data.len={}) - {}",
                    sock,
                    data_len,
                    err
                )
            },
        )?;
        Ok(())
    }
}
