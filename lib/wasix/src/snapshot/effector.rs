use std::{borrow::Cow, collections::LinkedList, ops::Range, sync::MutexGuard, time::SystemTime};

use bytes::Bytes;
use wasmer::{FunctionEnvMut, RuntimeError, WasmPtr};
use wasmer_types::MemorySize;
use wasmer_wasix_types::{
    types::__wasi_ciovec_t,
    wasi::{Errno, ExitCode, Fd},
};

use crate::{
    mem_error_to_wasi,
    os::task::process::WasiProcessInner,
    syscalls::{__asyncify_light, fd_write_internal, FdWriteSource},
    utils::map_snapshot_err,
    WasiEnv, WasiError, WasiRuntimeError, WasiThreadId,
};

use super::*;

#[derive(Debug, Clone)]
pub struct SnapshotEffector {}

#[cfg(feature = "snapshot")]
impl SnapshotEffector {
    pub fn save_write<M: MemorySize>(
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
                    .write(SnapshotLog::FileDescriptorWrite {
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

    pub async fn apply_write<M: MemorySize>(
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
            tracing::debug!(
                fd,
                offset,
                "restore error: failed to write to descriptor - {}",
                ret
            );
        }
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

    pub fn save_thread_state<M: MemorySize>(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        id: WasiThreadId,
        memory_stack: Bytes,
        rewind_stack: Bytes,
        store_data: Bytes,
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
                    store_data: Cow::Owned(store_data.into()),
                    is_64bit: M::is_64bit(),
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
        // We do not want the regions to be greater than 64KB as this will
        // otherwise create too much inefficiency. We choose 64KB as its
        // aligned with the standard WASM page size.
        let mut cur = 0u64;
        let mut regions = LinkedList::<Range<u64>>::new();
        while cur < memory.data_size() {
            let mut again = false;
            let mut end = memory.data_size().min(cur + 65536);
            for (_, thread) in process.threads.iter() {
                let layout = thread.memory_layout();
                if cur >= layout.stack_lower && cur < layout.stack_upper {
                    cur = layout.stack_upper;
                    again = true;
                    break;
                }
                if end > layout.stack_lower && end < layout.stack_upper {
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

    pub fn apply_memory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        region: Range<u64>,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let (env, mut store) = ctx.data_and_store_mut();
        let memory = unsafe { env.memory_view(&mut store) };
        memory
            .write(region.start, data.as_ref())
            .map_err(|err| WasiRuntimeError::Runtime(RuntimeError::user(err.into())))?;
        Ok(())
    }

    pub fn save_remove_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        let env = ctx.data();

        __asyncify_light(env, None, async {
            ctx.data()
                .runtime()
                .snapshot_capturer()
                .write(SnapshotLog::RemoveDirectory {
                    fd,
                    path: Cow::Owned(path),
                })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;

        Ok(())
    }

    pub fn apply_remove_directory(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: &str,
    ) -> anyhow::Result<()> {
        if let Err(err) = crate::syscalls::path_remove_directory_internal(ctx, fd, path) {
            tracing::debug!("restore error: failed to remove directory - {}", err);
        }
        Ok(())
    }

    pub fn save_unlink_file(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: String,
    ) -> anyhow::Result<()> {
        let env = ctx.data();

        __asyncify_light(env, None, async {
            ctx.data()
                .runtime()
                .snapshot_capturer()
                .write(SnapshotLog::UnlinkFile {
                    fd,
                    path: Cow::Owned(path),
                })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;

        Ok(())
    }

    pub fn apply_unlink_file(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        path: &str,
    ) -> anyhow::Result<()> {
        let ret = crate::syscalls::path_unlink_file_internal(ctx, fd, path)?;
        if ret != Errno::Success {
            tracing::debug!(fd, path, "restore error: failed to remove file - {}", ret);
        }
        Ok(())
    }

    pub fn save_rename(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_fd: Fd,
        old_path: String,
        new_fd: Fd,
        new_path: String,
    ) -> anyhow::Result<()> {
        let env = ctx.data();

        __asyncify_light(env, None, async {
            ctx.data()
                .runtime()
                .snapshot_capturer()
                .write(SnapshotLog::PathRename {
                    old_fd,
                    old_path: Cow::Owned(old_path),
                    new_fd,
                    new_path: Cow::Owned(new_path),
                })
                .await
                .map_err(map_snapshot_err)?;
            Ok(())
        })?
        .map_err(|err| WasiError::Exit(ExitCode::Errno(err)))?;

        Ok(())
    }

    pub fn apply_rename(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        old_fd: Fd,
        old_path: &str,
        new_fd: Fd,
        new_path: &str,
    ) -> anyhow::Result<()> {
        let ret = crate::syscalls::path_rename_internal(ctx, old_fd, old_path, new_fd, new_path)?;
        if ret != Errno::Success {
            tracing::debug!(
                old_fd,
                old_path,
                new_fd,
                new_path,
                "restore error: failed to rename path - {}",
                ret
            );
        }
        Ok(())
    }
}
