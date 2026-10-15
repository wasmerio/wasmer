//! What the kernel needs from the host: an executor, clocks, timers and entropy.
//!
//! The kernel only sees the [`Host`] and [`Executor`] traits and never names an async runtime.
//! Two implementations ship here: [`TokioHost`] for native embedders (feature `tokio`) and
//! [`WebHost`] for `wasm32-unknown-unknown` in a browser or node (feature `web`).

#[cfg(feature = "tokio")]
mod tokio_host;
#[cfg(feature = "web")]
mod web_host;

use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(feature = "tokio")]
pub use tokio_host::TokioHost;
#[cfg(feature = "web")]
pub use web_host::WebHost;

/// Boxed future without a `Send` bound, matching wasmer's async host calls.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A raw byte stream from the host, such as the embedder's stdin.
pub trait ByteSource: Send + Sync {
    /// Reads into `buf`; `Ok(0)` is end of stream.
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>>;
}

/// A raw byte stream to the host, such as the embedder's stdout.
pub trait ByteSink: Send + Sync {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>>;
}

/// The host streams a new process gets as descriptors 0, 1 and 2.
pub struct Stdio {
    pub stdin: Arc<dyn ByteSource>,
    pub stdout: Arc<dyn ByteSink>,
    pub stderr: Arc<dyn ByteSink>,
}

impl Stdio {
    /// Stdin at end of stream; stdout and stderr handed to callbacks, one call per guest write.
    /// Works on every host, and is the simplest way to wire a browser console or terminal.
    pub fn from_fns(
        stdout: impl Fn(&[u8]) + Send + Sync + 'static,
        stderr: impl Fn(&[u8]) + Send + Sync + 'static,
    ) -> Self {
        Self {
            stdin: Arc::new(EmptySource),
            stdout: Arc::new(FnSink(stdout)),
            stderr: Arc::new(FnSink(stderr)),
        }
    }
}

/// A source that is always at end of stream, like `/dev/null`.
pub struct EmptySource;

impl ByteSource for EmptySource {
    fn read<'a>(&'a self, _: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async { Ok(0) })
    }
}

/// A sink that hands every write to a callback and reports it fully written.
pub struct FnSink<F>(pub F);

impl<F: Fn(&[u8]) + Send + Sync> ByteSink for FnSink<F> {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        (self.0)(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

/// Clocks, timers, entropy and the executor that runs host work.
pub trait Host: Send + Sync {
    /// Time since the Unix epoch, for `CLOCK_REALTIME`.
    fn realtime(&self) -> Duration;
    /// Time since a fixed point in the past, for `CLOCK_MONOTONIC`. Never goes backwards.
    fn monotonic(&self) -> Duration;
    /// Completes once `duration` has passed, without blocking the calling thread.
    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()>;
    /// Completes once [`Host::monotonic`] reaches `deadline`, for absolute timeouts such as
    /// `clock_nanosleep(TIMER_ABSTIME)` on `CLOCK_MONOTONIC`. Never completes early.
    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async move {
            loop {
                let now = self.monotonic();
                if now >= deadline {
                    return;
                }
                self.sleep(deadline - now).await;
            }
        })
    }
    /// Fills `buf` with cryptographically secure random bytes.
    fn random(&self, buf: &mut [u8]);
    /// Where host work runs: background futures and blocking calls.
    fn executor(&self) -> &dyn Executor;
}

/// The work that runs a guest thread: called on the thread it runs on, it returns the future
/// that runs the thread to its end.
pub type ThreadStart = Box<dyn FnOnce() -> LocalFuture<'static, ()> + Send>;

/// Runs host work for the kernel, and guest threads.
pub trait Executor: Send + Sync {
    /// Runs `future` in the background on the current thread's executor. The future is not
    /// `Send`, because wasmer's async host calls are not; the caller must be running on that
    /// executor (see the implementation's docs). The future is detached: it runs until it
    /// completes or the executor shuts down.
    fn spawn_local(&self, future: LocalFuture<'static, ()>);

    /// Runs blocking `work` where it cannot stall the async executor, and completes once it
    /// returned. A panic in `work` resumes in the awaiting task. Prefer
    /// [`run_blocking`](Executor::run_blocking), which also returns the result.
    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()>;

    /// Runs a guest thread (a task created by `clone` with `CLONE_THREAD`) concurrently with the
    /// calling one, on a host thread or a worker; the host chooses. `start` is called where the
    /// thread runs, and the future it returns is driven there until it completes, so it may use
    /// [`spawn_local`](Executor::spawn_local) and [`spawn_blocking`](Executor::spawn_blocking).
    /// Fails when the host cannot start another thread; the kernel reports that as `EAGAIN`.
    /// The default supports no threads.
    fn spawn_thread(&self, start: ThreadStart) -> io::Result<()> {
        drop(start);
        Err(io::ErrorKind::Unsupported.into())
    }
}

impl dyn Executor + '_ {
    /// Runs blocking `work` off the async executor and returns its result. See
    /// [`Executor::spawn_blocking`].
    pub async fn run_blocking<T, F>(&self, work: F) -> T
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let slot = Arc::new(Mutex::new(None));
        let out = slot.clone();
        self.spawn_blocking(Box::new(move || {
            *out.lock().unwrap() = Some(work());
        }))
        .await;
        slot.lock()
            .unwrap()
            .take()
            .expect("spawn_blocking completed without running the work")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host whose clock moves only when it sleeps, to observe `sleep_until`'s requests.
    #[derive(Default)]
    struct SteppingHost {
        now: Mutex<Duration>,
        sleeps: Mutex<Vec<Duration>>,
    }

    impl Host for SteppingHost {
        fn realtime(&self) -> Duration {
            Duration::ZERO
        }

        fn monotonic(&self) -> Duration {
            *self.now.lock().unwrap()
        }

        // Wakes 1 ms early, as a coarse timer can, so `sleep_until` must sleep again.
        fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
            self.sleeps.lock().unwrap().push(duration);
            let step = duration
                .saturating_sub(Duration::from_millis(1))
                .max(Duration::from_millis(1));
            *self.now.lock().unwrap() += step;
            Box::pin(async {})
        }

        fn random(&self, _: &mut [u8]) {}

        fn executor(&self) -> &dyn Executor {
            unimplemented!()
        }
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        use std::task::{Context, Poll, Waker};
        let mut future = std::pin::pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("test futures complete immediately"),
        }
    }

    #[test]
    fn sleep_until_does_not_return_before_the_deadline() {
        let host = SteppingHost::default();
        block_on(host.sleep_until(Duration::from_millis(10)));
        assert!(host.monotonic() >= Duration::from_millis(10));
        assert_eq!(
            *host.sleeps.lock().unwrap(),
            [Duration::from_millis(10), Duration::from_millis(1)]
        );
    }

    #[test]
    fn sleep_until_a_past_deadline_does_not_sleep() {
        let host = SteppingHost::default();
        *host.now.lock().unwrap() = Duration::from_secs(5);
        block_on(host.sleep_until(Duration::from_secs(1)));
        assert!(host.sleeps.lock().unwrap().is_empty());
    }

    #[test]
    fn fn_sink_passes_every_write_through() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let out = written.clone();
        let stdio = Stdio::from_fns(
            move |data| out.lock().unwrap().extend_from_slice(data),
            |_| {},
        );
        assert_eq!(block_on(stdio.stdout.write(b"hi")).unwrap(), 2);
        assert_eq!(block_on(stdio.stdin.read(&mut [0; 4])).unwrap(), 0);
        assert_eq!(*written.lock().unwrap(), b"hi");
    }
}
