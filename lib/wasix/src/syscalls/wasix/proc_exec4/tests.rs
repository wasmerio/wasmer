use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, mpsc},
    task::{Context, Poll},
    thread,
    time::Duration,
};

use virtual_fs::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf, VirtualFile};
use wasmer::{Memory32, Module, Store, WasmPtr};

use super::*;

const DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct PendingExecutable {
    entered: Option<mpsc::Sender<()>>,
    release: tokio::sync::oneshot::Receiver<()>,
}

impl AsyncRead for PendingExecutable {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        Pin::new(&mut self.release).poll(cx).map(|_| Ok(()))
    }
}

impl AsyncWrite for PendingExecutable {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for PendingExecutable {
    fn start_seek(self: Pin<&mut Self>, _: io::SeekFrom) -> io::Result<()> {
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl VirtualFile for PendingExecutable {
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
        Err(virtual_fs::FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<(), virtual_fs::FsError> {
        Ok(())
    }

    fn poll_read_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(1))
    }

    fn poll_write_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
}

#[derive(Clone, Copy)]
enum SpawnMode {
    VforkExec,
    Exec,
    Spawn,
}

async fn assert_pending_executable_is_cancelled(mode: SpawnMode) {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (processes_tx, processes_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let runtime = tokio::runtime::Handle::current();
    let worker = thread::spawn(move || {
        let _runtime = runtime.enter();
        let result = {
            let mut store = Store::default();
            let module = Module::new(&store, "(module (memory (export \"memory\") 1))").unwrap();
            let fs = Arc::new(virtual_fs::mem_fs::FileSystem::default());
            fs.insert_device_file(
                "/pending.wasm".into(),
                Box::new(PendingExecutable {
                    entered: Some(entered_tx),
                    release: release_rx,
                }),
            )
            .unwrap();
            let (_instance, env) = WasiEnv::builder("cancel-vfork-exec")
                .engine(store.engine().clone())
                .fs(fs as Arc<dyn virtual_fs::FileSystem + Send + Sync>)
                .preopen_dir("/")
                .unwrap()
                .instantiate(module, &mut store)
                .unwrap();
            let parent = env.data(&store).process.clone();
            if matches!(mode, SpawnMode::VforkExec) {
                assert_eq!(
                    crate::syscalls::wasix::proc_fork_env::<Memory32>(
                        env.env.clone().into_mut(&mut store),
                        WasmPtr::new(0),
                    )
                    .unwrap(),
                    Errno::Success,
                );
                assert_ne!(parent.pid(), env.data(&store).pid());
                assert!(env.data(&store).vfork.is_some());
            }
            processes_tx.send(parent).unwrap();
            let ctx = env.env.clone().into_mut(&mut store);
            let mut name = "/pending.wasm".to_owned();
            match mode {
                SpawnMode::VforkExec | SpawnMode::Exec => {
                    proc_exec4_impl::<Memory32>(ctx, &mut name, Vec::new(), None, Bool::False, None)
                }
                SpawnMode::Spawn => crate::syscalls::wasix::proc_spawn3_impl::<Memory32>(
                    ctx,
                    &mut name,
                    Vec::new(),
                    None,
                    Vec::new(),
                    None,
                    Bool::False,
                    None,
                    WasmPtr::new(0),
                ),
            }
        };
        // Sending only after the Store and both environments drop also verifies
        // cancellation releases their thread handles, not just terminal status.
        let _ = done_tx.send(result);
    });

    let parent = processes_rx.recv_timeout(DEADLINE).unwrap();
    entered_rx
        .recv_timeout(DEADLINE)
        .expect("spawn did not enter its pending executable read");
    let children = parent.lock().children.clone();
    assert_eq!(
        children.len(),
        usize::from(!matches!(mode, SpawnMode::Exec))
    );
    parent.force_terminate(137.into()).unwrap();
    let result = done_rx.recv_timeout(DEADLINE);
    if result.is_err() {
        // Also make the uncancellable baseline finish before reporting failure.
        let _ = release_tx.send(());
        let _ = done_rx.recv_timeout(DEADLINE);
    }
    worker
        .join()
        .expect("cancelled executable loading panicked in the host");
    assert!(matches!(
        result.expect("cancelled executable loading retained its native task"),
        Err(WasiError::Exit(code)) if code == 137.into()
    ));
    assert_eq!(parent.active_threads(), 0);
    assert_eq!(parent.try_join().unwrap().unwrap(), 137.into());
    for child in children {
        assert_eq!(child.active_threads(), 0);
        assert_eq!(child.try_join().unwrap().unwrap(), 137.into());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_vfork_pending_exec_unwinds_without_panicking() {
    assert_pending_executable_is_cancelled(SpawnMode::VforkExec).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_pending_exec_releases_native_task() {
    assert_pending_executable_is_cancelled(SpawnMode::Exec).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_pending_spawn_releases_parent_and_child_tasks() {
    assert_pending_executable_is_cancelled(SpawnMode::Spawn).await;
}
