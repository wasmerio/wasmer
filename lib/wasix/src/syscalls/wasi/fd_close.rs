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

/// ### `fd_close()`
/// Close an open file descriptor
/// For sockets this will flush the data before the socket is closed
/// Inputs:
/// - `Fd fd`
///     A file descriptor mapping to an open file to close
/// Errors:
/// - `Errno::Isdir`
///     If `fd` is a directory
/// - `Errno::Badf`
///     If `fd` is invalid or not open
#[instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw(), %fd), ret)]
pub fn fd_close(mut ctx: FunctionEnvMut<'_, WasiEnv>, fd: WasiFd) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let env = ctx.data();
    let (_, mut state) = unsafe { env.get_memory_and_wasi_state(&ctx, 0) };
    let fd_entry = wasi_try_ok!(state.fs.get_fd(fd));

    // We don't want to allow programs that blindly close all FDs in a loop
    // to be able to close pre-opens, as that breaks wasix-libc in rather
    // spectacular fashion.
    if !fd_entry.is_stdio && fd_entry.inode.is_preopened {
        trace!("Skipping fd_close for pre-opened FD ({})", fd);
        return Ok(Errno::Success);
    }
    // Keep stdio behavior unchanged: flush before close.
    if fd <= __WASI_STDERR_FILENO {
        match __asyncify_light(env, None, state.fs.flush(fd))? {
            Ok(_) | Err(Errno::Isdir) | Err(Errno::Io) | Err(Errno::Access) => {}
            Err(e) => {
                return Ok(e);
            }
        }
        wasi_try_ok!(state.fs.close_fd(fd));
    } else {
        // Capture the file handle before removing the fd, then close first.
        // This avoids an fd-number reuse race where an async pre-close flush
        // can end up closing a newly allocated descriptor with the same number.
        let flush_target = {
            let guard = fd_entry.inode.read();
            match guard.deref() {
                Kind::File {
                    handle: Some(file), ..
                } => Some(file.clone()),
                _ => None,
            }
        };

        wasi_try_ok!(state.fs.close_fd(fd));

        if let Some(file) = flush_target {
            match __asyncify_light(env, None, FlushPoller { file })? {
                Ok(_) | Err(Errno::Isdir) | Err(Errno::Io) | Err(Errno::Access) => {}
                Err(e) => {
                    return Ok(e);
                }
            }
        }
    }

    #[cfg(feature = "journal")]
    if env.enable_journal {
        JournalEffector::save_fd_close(&mut ctx, fd).map_err(|err| {
            tracing::error!("failed to save close descriptor event - {}", err);
            WasiError::Exit(ExitCode::from(Errno::Fault))
        })?;
    }

    Ok(Errno::Success)
}
