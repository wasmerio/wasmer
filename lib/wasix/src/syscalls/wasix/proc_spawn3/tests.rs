use std::{
    future::Future,
    io,
    pin::Pin,
    sync::mpsc,
    task::{Context, Poll},
    thread,
    time::Duration,
};
use virtual_fs::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf, VirtualFile};
use wasmer::{Memory32, Module, Store};

use super::*;

const DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct PendingFlush {
    entered: Option<mpsc::Sender<()>>,
    release: tokio::sync::oneshot::Receiver<()>,
}

impl AsyncRead for PendingFlush {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for PendingFlush {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        Pin::new(&mut self.release).poll(cx).map(|_| Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl AsyncSeek for PendingFlush {
    fn start_seek(self: Pin<&mut Self>, _: io::SeekFrom) -> io::Result<()> {
        Ok(())
    }
    fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}
impl VirtualFile for PendingFlush {
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
    fn set_len(&mut self, _: u64) -> Result<(), virtual_fs::FsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), virtual_fs::FsError> {
        Ok(())
    }
    fn poll_read_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

fn assert_pending_flush_cancels(vfork_exit: bool) {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (process_tx, process_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let runtime = tokio::runtime::Handle::current();
    let worker = thread::spawn(move || {
        let _runtime = runtime.enter();
        let result = {
            let mut store = Store::default();
            let module = Module::new(&store, "(module (memory (export \"memory\") 1))").unwrap();
            let (instance, env) = WasiEnv::builder("pending-spawn-flush")
                .engine(store.engine().clone())
                .stdout(Box::new(PendingFlush {
                    entered: Some(entered_tx),
                    release: release_rx,
                }))
                .instantiate(module, &mut store)
                .unwrap();
            let parent = env.data(&store).process.clone();
            if vfork_exit {
                assert_eq!(
                    proc_fork_env::<Memory32>(
                        env.env.clone().into_mut(&mut store),
                        WasmPtr::new(0)
                    )
                    .unwrap(),
                    Errno::Success
                );
                let child = env.data(&store).process.clone();
                process_tx.send((parent, Some(child))).unwrap();
                proc_exit2::<Memory32>(env.env.clone().into_mut(&mut store), 0.into())
            } else {
                process_tx.send((parent, None)).unwrap();
                let memory = instance.exports.get_memory("memory").unwrap();
                let mut ctx = env.env.clone().into_mut(&mut store);
                let (data, store) = ctx.data_and_store_mut();
                apply_fd_op(
                    data,
                    &memory.view(&store),
                    &ProcSpawnFdOp::<Memory32> {
                        cmd: ProcSpawnFdOpName::Dup2,
                        fd: 1,
                        src_fd: 0,
                        name: 0,
                        name_len: 0,
                        dirflags: 0,
                        oflags: Oflags::empty(),
                        fs_rights_base: Rights::empty(),
                        fs_rights_inheriting: Rights::empty(),
                        fdflags: Fdflags::empty(),
                        fdflagsext: Fdflagsext::empty(),
                    },
                )
                .map(|result| assert_eq!(result, Ok(())))
            }
        };
        let _ = done_tx.send(result);
    });
    let (parent, child) = process_rx.recv_timeout(DEADLINE).unwrap();
    entered_rx
        .recv_timeout(DEADLINE)
        .expect("syscall did not poll pending flush");
    parent.force_terminate(137.into()).unwrap();
    let result = done_rx.recv_timeout(DEADLINE);
    if result.is_err() {
        let _ = release_tx.send(());
        let _ = done_rx.recv_timeout(DEADLINE);
    }
    worker
        .join()
        .expect("cancelled flush panicked instead of trapping");
    assert!(
        matches!(result.expect("terminated flush retained native task"), Err(WasiError::Exit(code)) if code == 137.into())
    );
    assert_eq!(parent.active_threads(), 0);
    if let Some(child) = child {
        assert_eq!(child.active_threads(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_cancels_spawn_dup2_flush() {
    assert_pending_flush_cancels(false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_cancels_vfork_exit_close_all() {
    assert_pending_flush_cancels(true);
}
