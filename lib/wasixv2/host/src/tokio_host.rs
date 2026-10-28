use std::{
    io,
    sync::Arc,
    time::{Duration, SystemTime},
};

use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    runtime::{Handle, RuntimeFlavor},
    sync::Mutex,
    task::LocalSet,
    time::Instant,
};

use crate::{BlockingSource, ByteSink, Executor, Host, LocalFuture, Stdio, ThreadStart};

/// The host on a tokio runtime with timers enabled: system clocks, tokio timers, the OS entropy
/// source, and tokio as the executor.
///
/// [`Executor::spawn_local`] uses `tokio::task::spawn_local`, so the kernel must be driven inside
/// a `tokio::task::LocalSet` (for example `LocalSet::block_on`). Blocking work runs on tokio's
/// blocking pool. Each guest thread gets its own OS thread with a `LocalSet` of its own, since
/// the futures that run guests are not `Send`, driven on the runtime of the thread that started
/// it when that is a multi-thread runtime, whose workers drive its timers and I/O for every
/// thread, so all guest threads share one runtime, its timers, I/O driver and blocking pool. A
/// current-thread runtime drives them only while its own thread waits in it, so on one (or
/// outside any runtime) a guest thread gets a current-thread runtime of its own.
pub struct TokioHost {
    // tokio's clock, so sleeps and `monotonic` agree, also when tests pause time.
    start: Instant,
}

impl Default for TokioHost {
    fn default() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Host for TokioHost {
    fn realtime(&self) -> Duration {
        // A host clock before 1970 is reported as the epoch.
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
    }

    fn monotonic(&self) -> Duration {
        self.start.elapsed()
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }

    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        // A deadline too far out to represent never arrives.
        match self.start.checked_add(deadline) {
            Some(deadline) => Box::pin(tokio::time::sleep_until(deadline)),
            None => Box::pin(std::future::pending()),
        }
    }

    fn random(&self, buf: &mut [u8]) {
        getrandom::fill(buf).expect("the host entropy source failed");
    }

    fn executor(&self) -> &dyn Executor {
        self
    }

    /// A memfd on Linux and an unlinked POSIX shared memory object on macOS
    /// ([`wasixv2_pages::Memfd`]); none elsewhere.
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    fn shared_pages(&self, size: u64) -> io::Result<Arc<dyn wasixv2_pages::SharedPages>> {
        Ok(Arc::new(wasixv2_pages::Memfd::new(size)?))
    }
}

impl Executor for TokioHost {
    fn spawn_local(&self, future: LocalFuture<'static, ()>) {
        tokio::task::spawn_local(future);
    }

    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        let handle = tokio::task::spawn_blocking(work);
        Box::pin(async move {
            if let Err(err) = handle.await {
                match err.try_into_panic() {
                    Ok(panic) => std::panic::resume_unwind(panic),
                    Err(err) => panic!("blocking host work did not run: {err}"),
                }
            }
        })
    }

    fn spawn_thread(&self, start: ThreadStart) -> io::Result<()> {
        let runtime = match Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
                GuestRuntime::Shared(handle)
            }
            _ => GuestRuntime::Own(
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?,
            ),
        };
        std::thread::Builder::new()
            .name("wasixv2-guest".into())
            .spawn(move || {
                let local = LocalSet::new();
                match runtime {
                    GuestRuntime::Shared(handle) => handle.block_on(local.run_until(start())),
                    GuestRuntime::Own(runtime) => local.block_on(&runtime, start()),
                }
            })?;
        Ok(())
    }
}

/// The runtime a guest thread's `LocalSet` runs on; see [`TokioHost`].
enum GuestRuntime {
    /// The multi-thread runtime of the thread that started it.
    Shared(Handle),
    /// One of its own.
    Own(tokio::runtime::Runtime),
}

impl Stdio {
    /// The host process's own stdin, stdout and stderr. Stdin is read on a thread of its own
    /// ([`BlockingSource`]), so no runtime waits for input at shutdown; stdout and stderr go
    /// through tokio.
    pub fn inherit() -> Self {
        Self {
            stdin: Arc::new(BlockingSource::new(std::io::stdin())),
            stdout: Arc::new(HostStream(Mutex::new(tokio::io::stdout()))),
            stderr: Arc::new(HostStream(Mutex::new(tokio::io::stderr()))),
        }
    }
}

struct HostStream<T>(Mutex<T>);

impl<T: AsyncWrite + Unpin + Send> ByteSink for HostStream<T> {
    // Flushed per call, so output reaches the host in the order the guest wrote it.
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async move {
            let mut stream = self.0.lock().await;
            let n = stream.write(data).await?;
            stream.flush().await?;
            Ok(n)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn sleep_until_waits_for_the_monotonic_deadline() {
        let host = TokioHost::default();
        let deadline = host.monotonic() + Duration::from_secs(3);
        host.sleep_until(deadline).await;
        assert!(host.monotonic() >= deadline);
    }

    #[tokio::test]
    async fn spawn_local_and_run_blocking() {
        let host = TokioHost::default();
        LocalSet::new()
            .run_until(async {
                let ran = Rc::new(Cell::new(false));
                let flag = ran.clone();
                host.executor()
                    .spawn_local(Box::pin(async move { flag.set(true) }));
                tokio::task::yield_now().await;
                assert!(ran.get());
                let thread = host
                    .executor()
                    .run_blocking(|| std::thread::current().id())
                    .await;
                assert_ne!(thread, std::thread::current().id());
            })
            .await;
    }

    #[test]
    fn spawn_thread_runs_on_another_thread_with_its_own_executor() {
        let host = Arc::new(TokioHost::default());
        let (done, finished) = std::sync::mpsc::channel();
        let caller = std::thread::current().id();
        let thread_host = host.clone();
        host.spawn_thread(Box::new(move || {
            Box::pin(async move {
                let (tx, rx) = tokio::sync::oneshot::channel();
                thread_host
                    .executor()
                    .spawn_local(Box::pin(async move { tx.send(()).unwrap() }));
                rx.await.unwrap();
                thread_host.sleep(Duration::from_millis(1)).await;
                done.send(std::thread::current().id()).unwrap();
            })
        }))
        .unwrap();
        let thread = finished.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_ne!(thread, caller);
    }

    /// On a multi-thread runtime, a guest thread runs on that runtime, not one of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_thread_shares_a_multi_thread_runtime() {
        let host = Arc::new(TokioHost::default());
        let (tx, rx) = tokio::sync::oneshot::channel();
        let thread_host = host.clone();
        host.spawn_thread(Box::new(move || {
            Box::pin(async move {
                thread_host.sleep(Duration::from_millis(1)).await;
                tx.send(Handle::current().runtime_flavor()).unwrap();
            })
        }))
        .unwrap();
        let flavor = tokio::time::timeout(Duration::from_secs(10), rx).await;
        assert_eq!(flavor.unwrap().unwrap(), RuntimeFlavor::MultiThread);
    }

    #[tokio::test]
    #[should_panic(expected = "boom")]
    async fn run_blocking_resumes_panics() {
        TokioHost::default()
            .executor()
            .run_blocking(|| panic!("boom"))
            .await
    }
}
