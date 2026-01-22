use super::*;
use crate::syscalls::*;
use std::{future::Future, pin::Pin, sync::Arc, task::Context, task::Poll};
use virtual_fs::VirtualFile;

struct FlushPoller {
    file: Arc<std::sync::RwLock<Box<dyn VirtualFile + Send + Sync>>>,
}

impl Future for FlushPoller {
    type Output = Result<(), Errno>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut file = self.file.write().unwrap();
        Pin::new(file.as_mut())
            .poll_flush(cx)
            .map_err(|_| Errno::Io)
    }
}

/// ### `fd_renumber()`
/// Atomically copy file descriptor
/// Inputs:
/// - `Fd from`
///     File descriptor to copy
/// - `Fd to`
///     Location to copy file descriptor to
#[instrument(level = "trace", skip_all, fields(%from, %to), ret)]
pub fn fd_renumber(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    from: WasiFd,
    to: WasiFd,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let ret = fd_renumber_internal(&mut ctx, from, to)?;
    let env = ctx.data();

    if ret == Errno::Success {
        #[cfg(feature = "journal")]
        if env.enable_journal {
            JournalEffector::save_fd_renumber(&mut ctx, from, to).map_err(|err| {
                tracing::error!("failed to save file descriptor renumber event - {}", err);
                WasiError::Exit(ExitCode::from(Errno::Fault))
            })?;
        }
    }

    Ok(ret)
}

pub(crate) fn fd_renumber_internal(
    ctx: &mut FunctionEnvMut<'_, WasiEnv>,
    from: WasiFd,
    to: WasiFd,
) -> Result<Errno, WasiError> {
    if from == to {
        return Ok(Errno::Success);
    }
    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };

    // Hold a single write lock for both the remove and insert to prevent
    // another thread from allocating into the target slot between the two
    // operations.
    let old_fd;
    {
        let mut fd_map = state.fs.fd_map.write().unwrap();
        // Validate the source first. If `from` is invalid we must not mutate `to`.
        let fd_entry = wasi_try_ok!(fd_map.get(from).ok_or(Errno::Badf));

        // Never allow renumbering over preopens.
        if let Some(target_fd) = fd_map.get(to)
            && !target_fd.is_stdio
            && target_fd.inode.is_preopened
        {
            warn!("Refusing fd_renumber({from}, {to}) because FD {to} is pre-opened");
            return Ok(Errno::Notsup);
        }

        let new_fd_entry = Fd {
            inner: FdInner {
                offset: fd_entry.inner.offset.clone(),
                rights: fd_entry.inner.rights_inheriting,
                fd_flags: {
                    let mut f = fd_entry.inner.fd_flags;
                    f.set(Fdflagsext::CLOEXEC, false);
                    f
                },
                ..fd_entry.inner
            },
            inode: fd_entry.inode.clone(),
            ..*fd_entry
        };

        // Remove the target FD under the same lock (replaces the separate
        // close_fd call which would acquire its own lock).
        old_fd = fd_map.remove(to);

        if !fd_map.insert(true, to, new_fd_entry) {
            panic!("Internal error: expected FD {to} to be free after closing in fd_renumber");
        }

        fd_map.remove(from);
    }
    // Flush and drop the old FD outside the lock. The flush is best-effort:
    // failures are intentionally ignored so fd_renumber result depends only on
    // descriptor map updates and validation.
    let flush_target = old_fd.as_ref().and_then(|fd_entry| {
        let guard = fd_entry.inode.read();
        match guard.deref() {
            Kind::File {
                handle: Some(file), ..
            } => Some(file.clone()),
            _ => None,
        }
    });
    drop(old_fd);

    if let Some(file) = flush_target {
        let _ = __asyncify_light(env, None, FlushPoller { file })?;
    }

    Ok(Errno::Success)
}
