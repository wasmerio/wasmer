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

fn close_fd_and_prepare_flush(
    fs: &crate::fs::WasiFs,
    fd: WasiFd,
    fd_entry: &crate::fs::Fd,
) -> Result<Option<FlushPoller>, Errno> {
    let flush_target = if fd == __WASI_STDIN_FILENO {
        None
    } else {
        let guard = fd_entry.inode.read();
        match guard.deref() {
            Kind::File {
                handle: Some(file), ..
            } => Some(file.clone()),
            _ => None,
        }
    };

    fs.close_fd(fd)?;

    Ok(flush_target.map(|file| FlushPoller { file }))
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
    // Capture the file handle before removing the fd, then close first.
    // This avoids an fd-number reuse race where an async pre-close flush
    // can end up closing a newly allocated descriptor with the same number.
    if let Some(flush_poller) = wasi_try_ok!(close_fd_and_prepare_flush(&state.fs, fd, &fd_entry)) {
        match __asyncify_light(env, None, flush_poller)? {
            Ok(_) | Err(Errno::Isdir) | Err(Errno::Io) | Err(Errno::Access) => {}
            Err(e) => {
                return Ok(e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{Fd, FdInner, InodeVal, Kind, WasiFs, WasiFsRoot, WasiInodes};
    use crate::state::ALL_RIGHTS;
    use std::io;
    use std::ops::{Deref, DerefMut};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, RwLock};
    use std::task::Waker;
    use std::time::{Duration, Instant};
    use virtual_fs::{AsyncRead, AsyncSeek, AsyncWrite, FileSystem, TmpFileSystem};
    use wasmer_wasix_types::wasi::{Fdflags, Fdflagsext};

    #[derive(Debug)]
    struct BlockingFlushFile {
        release_flush: Arc<AtomicBool>,
        flush_entered: Arc<AtomicBool>,
        waker: Arc<Mutex<Option<Waker>>>,
    }

    impl BlockingFlushFile {
        fn new(
            release_flush: Arc<AtomicBool>,
            flush_entered: Arc<AtomicBool>,
            waker: Arc<Mutex<Option<Waker>>>,
        ) -> Self {
            Self {
                release_flush,
                flush_entered,
                waker,
            }
        }
    }

    impl VirtualFile for BlockingFlushFile {
        fn last_accessed(&self) -> u64 {
            0
        }

        fn last_modified(&self) -> u64 {
            0
        }

        fn created_time(&self) -> u64 {
            0
        }

        fn size(&self) -> u64 {
            0
        }

        fn set_len(&mut self, _new_size: u64) -> Result<(), crate::FsError> {
            Err(crate::FsError::PermissionDenied)
        }

        fn unlink(&mut self) -> Result<(), crate::FsError> {
            Ok(())
        }

        fn is_open(&self) -> bool {
            true
        }

        fn get_special_fd(&self) -> Option<u32> {
            None
        }

        fn poll_read_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(0))
        }

        fn poll_write_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(8192))
        }
    }

    impl AsyncRead for BlockingFlushFile {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for BlockingFlushFile {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            self.flush_entered.store(true, Ordering::SeqCst);
            if self.release_flush.load(Ordering::SeqCst) {
                Poll::Ready(Ok(()))
            } else {
                *self.waker.lock().unwrap() = Some(cx.waker().clone());
                Poll::Pending
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncSeek for BlockingFlushFile {
        fn start_seek(self: Pin<&mut Self>, _position: std::io::SeekFrom) -> std::io::Result<()> {
            Ok(())
        }

        fn poll_complete(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<u64>> {
            Poll::Ready(Ok(0))
        }
    }

    impl std::io::Read for BlockingFlushFile {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }

    impl std::io::Write for BlockingFlushFile {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl std::io::Seek for BlockingFlushFile {
        fn seek(&mut self, _pos: std::io::SeekFrom) -> io::Result<u64> {
            Ok(0)
        }
    }

    #[derive(Debug, Default)]
    struct MemoryFile(Vec<u8>);

    impl VirtualFile for MemoryFile {
        fn last_accessed(&self) -> u64 {
            0
        }

        fn last_modified(&self) -> u64 {
            0
        }

        fn created_time(&self) -> u64 {
            0
        }

        fn size(&self) -> u64 {
            self.0.len() as u64
        }

        fn set_len(&mut self, _new_size: u64) -> Result<(), crate::FsError> {
            Err(crate::FsError::PermissionDenied)
        }

        fn unlink(&mut self) -> Result<(), crate::FsError> {
            Ok(())
        }

        fn is_open(&self) -> bool {
            true
        }

        fn get_special_fd(&self) -> Option<u32> {
            None
        }

        fn poll_read_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(0))
        }

        fn poll_write_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(8192))
        }
    }

    impl AsyncRead for MemoryFile {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for MemoryFile {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.0.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncSeek for MemoryFile {
        fn start_seek(self: Pin<&mut Self>, _position: std::io::SeekFrom) -> std::io::Result<()> {
            Ok(())
        }

        fn poll_complete(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<u64>> {
            Poll::Ready(Ok(0))
        }
    }

    impl std::io::Read for MemoryFile {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Ok(0)
        }
    }

    impl std::io::Write for MemoryFile {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl std::io::Seek for MemoryFile {
        fn seek(&mut self, _pos: std::io::SeekFrom) -> io::Result<u64> {
            Ok(0)
        }
    }

    fn wait_for_flag(flag: &AtomicBool, label: &str) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while !flag.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline, "timed out waiting for {label}");
            std::thread::yield_now();
        }
    }

    fn install_fd(fs: &WasiFs, from: u32, to: u32) {
        let old_fd = {
            let mut fd_map = fs.fd_map.write().unwrap();
            let fd_entry = fd_map.get(from).cloned().expect("source fd should exist");
            let new_fd_entry = Fd {
                inner: FdInner {
                    offset: fd_entry.inner.offset.clone(),
                    rights: fd_entry.inner.rights_inheriting,
                    fd_flags: {
                        let mut flags = fd_entry.inner.fd_flags;
                        flags.set(Fdflagsext::CLOEXEC, false);
                        flags
                    },
                    ..fd_entry.inner
                },
                inode: fd_entry.inode.clone(),
                ..fd_entry
            };

            let old_fd = fd_map.remove(to);
            assert!(
                fd_map.insert(true, to, new_fd_entry),
                "target fd should be free after removal"
            );
            old_fd
        };

        drop(old_fd);
    }

    #[test]
    fn stdio_close_does_not_remove_replacement_fd() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create setup runtime");
        let _guard = rt.enter();
        let inodes = WasiInodes::new();
        let root_fs = WasiFsRoot::from_filesystem(
            Arc::new(TmpFileSystem::new()) as Arc<dyn FileSystem + Send + Sync>
        );
        let fs = Arc::new(
            WasiFs::new_with_preopen(&inodes, &[], &[], root_fs).expect("failed to create WasiFs"),
        );

        let release_flush = Arc::new(AtomicBool::new(false));
        let flush_entered = Arc::new(AtomicBool::new(false));
        let flush_waker = Arc::new(Mutex::new(None));
        fs.swap_file(
            1,
            Box::new(BlockingFlushFile::new(
                release_flush.clone(),
                flush_entered.clone(),
                flush_waker.clone(),
            )),
        )
        .expect("failed to install blocking stdout");

        let replacement_inode = inodes.add_inode_val(InodeVal {
            stat: RwLock::new(Default::default()),
            is_preopened: false,
            name: RwLock::new("replacement".into()),
            kind: RwLock::new(Kind::File {
                handle: Some(Arc::new(RwLock::new(Box::new(MemoryFile::default())))),
                path: PathBuf::new(),
                fd: None,
            }),
        });
        fs.create_fd_ext(
            ALL_RIGHTS,
            ALL_RIGHTS,
            Fdflags::empty(),
            Fdflagsext::empty(),
            Fd::READ | Fd::WRITE,
            replacement_inode,
            Some(10),
            true,
        )
        .expect("failed to create replacement fd");

        let fs_for_close = fs.clone();
        let close_thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create runtime");
            let fd_entry = fs_for_close.get_fd(1).expect("fd 1 should exist");
            let flush_poller =
                close_fd_and_prepare_flush(&fs_for_close, 1, &fd_entry).expect("close failed");
            if let Some(flush_poller) = flush_poller {
                rt.block_on(flush_poller).expect("flush failed");
            }
        });

        wait_for_flag(&flush_entered, "stdout flush to start");
        install_fd(&fs, 10, 1);

        release_flush.store(true, Ordering::SeqCst);
        if let Some(waker) = flush_waker.lock().unwrap().deref_mut().take() {
            waker.wake();
        }
        close_thread.join().expect("close thread panicked");

        assert!(
            fs.get_fd(1).is_ok(),
            "fd 1 replacement was removed by the delayed stdio close"
        );
    }
}
