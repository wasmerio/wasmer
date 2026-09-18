use std::{
    future::{Future, poll_fn},
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll},
    thread,
    time::Duration,
};

use virtual_fs::{
    AsyncRead, AsyncReadExt, AsyncSeek, AsyncWrite, AsyncWriteExt, Pipe, ReadBuf, VirtualFile,
};
use virtual_mio::block_on;
use wasmer::{Module, Store};

use super::{__asyncify_light, __sock_asyncify};
use crate::{
    WasiEnv, WasiError,
    fs::{Fd, Kind},
    net::socket::{InodeSocket, InodeSocketKind, SocketProperties},
    syscalls::{Addressfamily, Errno, ExitCode, Fdflags, Fdflagsext, Rights, SockProto, Socktype},
};

const CANCEL_TIMEOUT: Duration = Duration::from_secs(2);

struct NotifyOnDrop(Option<mpsc::Sender<()>>);

impl Drop for NotifyOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

struct NotifyFirstPoll<F> {
    inner: Pin<Box<F>>,
    entered: Option<mpsc::Sender<()>>,
}

impl<F: Future> Future for NotifyFirstPoll<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(sender) = self.entered.take() {
            let _ = sender.send(());
        }
        self.inner.as_mut().poll(cx)
    }
}

fn assert_forced_exit<T: std::fmt::Debug>(result: crate::WasiResult<T>, expected: ExitCode) {
    match result {
        Err(WasiError::Exit(actual)) => assert_eq!(actual, expected),
        other => panic!("pending native wait did not exit with the forced status: {other:?}"),
    }
}

fn test_env(name: &str) -> WasiEnv {
    let store = Store::default();
    WasiEnv::builder(name)
        .engine(store.engine().clone())
        .build()
        .unwrap()
}

fn assert_native_wait_times_out(timeout: Duration, expected: Errno) {
    let env = test_env("native-wait-timeout");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut release_rx = Box::pin(release_rx);
        let work = poll_fn(move |cx| {
            let _ = entered_tx.send(());
            release_rx.as_mut().poll(cx).map(|_| Ok::<(), Errno>(()))
        });
        let _ = result_tx.send(__asyncify_light(&env, Some(timeout), work));
    });

    entered_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("timed native wait never polled its work");
    let result = match result_rx.recv_timeout(CANCEL_TIMEOUT) {
        Ok(result) => result,
        Err(error) => {
            // The old implementation ignored the timeout. Release its work so
            // this regression can fail without leaving a native thread behind.
            let _ = release_tx.send(());
            let _ = result_rx.recv_timeout(CANCEL_TIMEOUT);
            worker.join().unwrap();
            panic!("native wait ignored its {timeout:?} timeout: {error}");
        }
    };
    assert_eq!(result.unwrap(), Err(expected));
    worker.join().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_wait_preserves_ready_timeout_and_preterminated_semantics() {
    let env = test_env("native-wait-semantics");
    assert_eq!(
        __asyncify_light(&env, None, async { Ok::<_, Errno>(7u32) }).unwrap(),
        Ok(7)
    );
    assert_native_wait_times_out(Duration::ZERO, Errno::Again);
    assert_native_wait_times_out(Duration::from_millis(10), Errno::Timedout);

    let preterminated = test_env("native-wait-preterminated");
    let expected = ExitCode::from(137);
    preterminated.process.force_terminate(expected).unwrap();
    let polled = Arc::new(AtomicBool::new(false));
    let polled_by_work = polled.clone();
    let result = __asyncify_light(&preterminated, None, async move {
        polled_by_work.store(true, Ordering::SeqCst);
        Ok::<_, Errno>(())
    });
    assert_forced_exit(result, expected);
    assert!(
        !polled.load(Ordering::SeqCst),
        "preterminated work was polled before cancellation won"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_cancels_pending_pipe_read_and_drops_its_future() {
    let env = test_env("cancel-pending-pipe-read");
    let process = env.process.clone();
    let (mut cleanup_writer, mut reader) = Pipe::channel();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();

    let worker = thread::spawn(move || {
        let work = async move {
            let _drop = NotifyOnDrop(Some(dropped_tx));
            let mut byte = [0u8; 1];
            let read = reader.read(&mut byte);
            NotifyFirstPoll {
                inner: Box::pin(read),
                entered: Some(entered_tx),
            }
            .await
            .map_err(|_| Errno::Io)
        };
        let result = __asyncify_light(&env, None, work);
        let _ = result_tx.send(result);
    });

    entered_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("pipe read never reached its first pending poll");
    let expected = ExitCode::from(137);
    process.force_terminate(expected).unwrap();

    let result = match result_rx.recv_timeout(CANCEL_TIMEOUT) {
        Ok(result) => result,
        Err(error) => {
            // Make a broken implementation finish before failing so the test never
            // strands a native worker in the suite.
            block_on(cleanup_writer.write_all(b"x")).unwrap();
            let _ = result_rx.recv_timeout(CANCEL_TIMEOUT);
            worker.join().unwrap();
            panic!("forced termination did not cancel the pending pipe read: {error}");
        }
    };
    assert_forced_exit(result, expected);
    dropped_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("the cancelled pipe-read future retained its resources");
    worker.join().unwrap();
}

#[derive(Debug)]
struct PendingReadFile {
    entered: Option<mpsc::Sender<()>>,
    release: tokio::sync::oneshot::Receiver<()>,
}

impl VirtualFile for PendingReadFile {
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

    fn set_len(&mut self, _new_size: u64) -> Result<(), virtual_fs::FsError> {
        Err(virtual_fs::FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<(), virtual_fs::FsError> {
        Ok(())
    }

    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        match self.poll_release(cx) {
            Poll::Ready(()) => Poll::Ready(Ok(0)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

impl PendingReadFile {
    fn poll_release(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(sender) = self.entered.take() {
            let _ = sender.send(());
        }
        Pin::new(&mut self.release).poll(cx).map(|_| ())
    }
}

impl AsyncRead for PendingReadFile {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.poll_release(cx).map(|()| Ok(()))
    }
}

impl AsyncWrite for PendingReadFile {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for PendingReadFile {
    fn start_seek(self: Pin<&mut Self>, _position: io::SeekFrom) -> io::Result<()> {
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_unwinds_guest_blocked_in_fd_read() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (process_tx, process_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let runtime = tokio::runtime::Handle::current();
    let worker = thread::spawn(move || {
        let _runtime = runtime.enter();
        let _drop = NotifyOnDrop(Some(dropped_tx));
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "wasi_snapshot_preview1" "fd_read"
                    (func $fd_read (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "_start")
                    (i32.store (i32.const 0) (i32.const 16))
                    (i32.store (i32.const 4) (i32.const 1))
                    (drop (call $fd_read
                        (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 8)))))"#,
        )
        .unwrap();
        let (instance, env) = WasiEnv::builder("guest-pending-fd-read")
            .engine(store.engine().clone())
            .stdin(Box::new(PendingReadFile {
                entered: Some(entered_tx),
                release: release_rx,
            }))
            .instantiate(module, &mut store)
            .unwrap();
        process_tx.send(env.data(&store).process.clone()).unwrap();
        let start = instance
            .exports
            .get_typed_function::<(), ()>(&store, "_start")
            .unwrap();
        let _ = result_tx.send(start.call(&mut store).is_err());
    });

    let process = process_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("guest process was not instantiated");
    entered_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("guest fd_read never reached the pending host file");
    let expected = ExitCode::from(137);
    process.force_terminate(expected).unwrap();

    let result = match result_rx.recv_timeout(CANCEL_TIMEOUT) {
        Ok(result) => result,
        Err(error) => {
            let _ = release_tx.send(());
            let _ = result_rx.recv_timeout(CANCEL_TIMEOUT);
            worker.join().unwrap();
            panic!("forced termination did not unwind the guest fd_read: {error}");
        }
    };
    assert!(
        result,
        "forced guest fd_read unexpectedly returned normally"
    );
    assert_eq!(process.try_join().unwrap().unwrap(), expected);
    dropped_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("the guest callback retained its native task resources");
    worker.join().unwrap();
}

fn test_socket_properties() -> SocketProperties {
    SocketProperties {
        family: Addressfamily::Inet4,
        ty: Socktype::Stream,
        pt: SockProto::Tcp,
        only_v6: false,
        reuse_port: false,
        reuse_addr: false,
        no_delay: None,
        keep_alive: None,
        dont_route: None,
        send_buf_size: None,
        recv_buf_size: None,
        write_timeout: None,
        read_timeout: None,
        accept_timeout: None,
        connect_timeout: None,
        handler: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_terminate_cancels_pending_native_socket_actor() {
    let env = test_env("cancel-pending-socket-actor");
    let process = env.process.clone();
    let state = env.state();
    let inode = state.fs.create_inode_with_default_stat(
        &state.inodes,
        Kind::Socket {
            socket: InodeSocket::new(InodeSocketKind::PreSocket {
                props: test_socket_properties(),
                addr: None,
            }),
        },
        false,
        "test-socket".into(),
    );
    let socket_fd = state
        .fs
        .create_fd(
            Rights::all_socket(),
            Rights::all_socket(),
            Fdflags::empty(),
            Fdflagsext::empty(),
            Fd::READ | Fd::WRITE,
            inode,
        )
        .unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();

    let worker = thread::spawn(move || {
        let result = __sock_asyncify(&env, socket_fd, Rights::empty(), move |_, _| async move {
            let _drop = NotifyOnDrop(Some(dropped_tx));
            let mut release_rx = Box::pin(release_rx);
            poll_fn(move |cx| {
                let _ = entered_tx.send(());
                release_rx.as_mut().poll(cx).map(|_| Ok::<(), Errno>(()))
            })
            .await
        });
        let _ = result_tx.send(result);
    });

    entered_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("socket actor never reached its pending poll");
    let expected = ExitCode::from(137);
    process.force_terminate(expected).unwrap();

    let result = match result_rx.recv_timeout(CANCEL_TIMEOUT) {
        Ok(result) => result,
        Err(error) => {
            let _ = release_tx.send(());
            let _ = result_rx.recv_timeout(CANCEL_TIMEOUT);
            worker.join().unwrap();
            panic!("forced termination did not cancel the pending socket actor: {error}");
        }
    };
    assert_forced_exit(result, expected);
    dropped_rx
        .recv_timeout(CANCEL_TIMEOUT)
        .expect("the cancelled socket actor retained its resources");
    worker.join().unwrap();
}
