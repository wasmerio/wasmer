use std::{borrow::Cow, collections::LinkedList, ops::Range, sync::MutexGuard, time::SystemTime};

use bytes::Bytes;
use virtual_fs::AsyncWriteExt;
use wasmer::{FunctionEnvMut, WasmPtr};
use wasmer_types::MemorySize;
use wasmer_wasix_types::{types::__wasi_ciovec_t, wasi::ExitCode};

use crate::{
    fs::fs_error_into_wasi_err,
    mem_error_to_wasi,
    os::task::process::WasiProcessInner,
    syscalls::__asyncify_light,
    utils::{map_io_err, map_snapshot_err},
    WasiEnv, WasiError, WasiThreadId,
};

use super::*;

#[derive(Debug, Clone)]
pub struct SnapshotEffector {}

#[cfg(feature = "snapshot")]
impl SnapshotEffector {
    pub fn save_terminal_data<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        iovs: WasmPtr<__wasi_ciovec_t<M>, M>,
        iovs_len: M::Offset,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        let memory = unsafe { env.memory_view(&ctx) };
        let iovs_arr = iovs.slice(&memory, iovs_len)?;

        __asyncify_light(env, None, async {
            let iovs_arr = iovs_arr.access().map_err(mem_error_to_wasi)?;
            for iovs in iovs_arr.iter() {
                let buf = WasmPtr::<u8, M>::new(iovs.buf)
                    .slice(&memory, iovs.buf_len)
                    .map_err(mem_error_to_wasi)?
                    .access()
                    .map_err(mem_error_to_wasi)?;
                ctx.data()
                    .runtime()
                    .snapshot_capturer()
                    .write(SnapshotLog::TerminalData {
                        data: Cow::Borrowed(buf.as_ref()),
                    })
                    .await
                    .map_err(map_snapshot_err)?;
            }
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;
        Ok(())
    }

    pub fn apply_terminal_data<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        __asyncify_light(env, None, async {
            if let Some(mut stdout) = ctx.data().stdout().map_err(fs_error_into_wasi_err)? {
                stdout.write_all(data).await.map_err(map_io_err)?;
            }
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;
        Ok(())
    }

    pub fn save_thread_exit(
        env: &WasiEnv,
        id: WasiThreadId,
        exit_code: Option<ExitCode>,
    ) -> anyhow::Result<()> {
        __asyncify_light(env, None, async {
            env.runtime()
                .snapshot_capturer()
                .write(SnapshotLog::CloseThread { id, exit_code })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;
        Ok(())
    }

    pub fn save_thread_state(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        id: WasiThreadId,
        memory_stack: Bytes,
        rewind_stack: Bytes,
    ) -> anyhow::Result<()> {
        let env = ctx.data();

        __asyncify_light(env, None, async {
            ctx.data()
                .runtime()
                .snapshot_capturer()
                .write(SnapshotLog::SetThread {
                    id,
                    call_stack: Cow::Owned(rewind_stack.into()),
                    memory_stack: Cow::Owned(memory_stack.into()),
                })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;

        Ok(())
    }

    pub fn save_memory_and_snapshot(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        process: &mut MutexGuard<'_, WasiProcessInner>,
        trigger: SnapshotTrigger,
    ) -> anyhow::Result<()> {
        let env = ctx.data();
        let memory = unsafe { env.memory_view(ctx) };

        // Compute all the regions that we need to save which is basically
        // everything in the memory except for the memory stacks.
        //
        // We do not want the regions to be greater than 128KB as this will
        // otherwise create too much inefficiency.
        let mut cur = 0u64;
        let mut regions = LinkedList::<Range<u64>>::new();
        while cur < memory.data_size() {
            let mut again = false;
            let mut end = memory.data_size().min(cur + 131_072);
            for (_, thread) in process.threads.iter() {
                let layout = thread.memory_layout();
                if cur >= layout.stack_lower && cur < layout.stack_upper {
                    cur = layout.stack_upper;
                    again = true;
                    break;
                }
                if end > layout.stack_lower {
                    end = end.min(layout.stack_lower);
                }
            }
            if again {
                continue;
            }
            regions.push_back(cur..end);
            cur = end;
        }

        // Now that we known all the regions that need to be saved we
        // enter a processing loop that dumps all the data to the log
        // file in an orderly manner.
        __asyncify_light(env, None, async {
            let memory = unsafe { env.memory_view(ctx) };
            let capturer = ctx.data().runtime().snapshot_capturer();

            for region in regions {
                // We grab this region of memory as a vector and hash
                // it, which allows us to make some logging efficiency
                // gains.
                let data = memory
                    .copy_range_to_vec(region.clone())
                    .map_err(mem_error_to_wasi)?;

                // Now we write it to the snap snapshot capturer
                capturer
                    .write(SnapshotLog::UpdateMemoryRegion {
                        region,
                        data: data.into(),
                    })
                    .await
                    .map_err(map_snapshot_err)?;
            }

            // Finally we mark the end of the snapshot so that
            // it can act as a restoration point
            let when = SystemTime::now();
            capturer
                .write(SnapshotLog::Snapshot { when, trigger })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;
        Ok(())
    }
}
