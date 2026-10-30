//! What the kernel needs from the host: an executor, clocks, timers, entropy and shared memory
//! objects.
//!
//! The kernel only sees the [`Host`] and [`Executor`] traits and never names an async runtime.
//! Two implementations ship here: [`TokioHost`] for native embedders (feature `tokio`) and
//! [`WebHost`] for `wasm32-unknown-unknown` in a browser or node (feature `web`).

#[cfg(feature = "browser")]
pub mod browser;
#[cfg(feature = "tokio")]
mod timer;
#[cfg(feature = "tokio")]
mod tokio_host;
#[cfg(feature = "web")]
mod web_host;
#[cfg(feature = "web")]
mod web_timer;

use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

#[cfg(feature = "tokio")]
pub use tokio_host::TokioHost;
use wasixv2_pages::SharedPages;
#[cfg(feature = "browser")]
pub use web_host::BrowserHost;
#[cfg(feature = "web")]
pub use web_host::WebHost;

/// Boxed future without a `Send` bound, matching wasmer's async host calls.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A raw byte stream from the host, such as the embedder's stdin.
///
/// Readiness-based, like `AsyncRead`, so the kernel can tell when input is available without
/// waiting for it: `poll` and `epoll` on the descriptor, `O_NONBLOCK` reads, and blocked reads
/// that a signal interrupts.
pub trait ByteSource: Send + Sync {
    /// Reads into `buf` without blocking. `Ready(Ok(0))` is end of stream; a later read may
    /// find more input, as on a terminal after `^D`. `Pending` means no input yet: it consumes
    /// nothing, and the source wakes `cx`'s waker, from any thread, once a read may make
    /// progress.
    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>>;
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

/// A terminal the host gives the machine as its console: the host's side of a terminal in raw
/// mode, whose bytes the kernel's line discipline handles (echo, line editing, `^C`). The
/// input's end means the terminal went away, as a line that drops: the console hangs up.
#[derive(Clone)]
pub struct Console {
    /// What is typed, byte by byte.
    pub input: Arc<dyn ByteSource>,
    /// What the terminal shows.
    pub output: Arc<dyn ByteSink>,
    /// Its size when the machine starts.
    pub size: WindowSize,
}

/// A terminal's window size (`struct winsize`): rows and columns of characters, and pixels,
/// 0 where unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowSize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

/// A source that is always at end of stream, like `/dev/null`.
pub struct EmptySource;

impl ByteSource for EmptySource {
    fn poll_read(&self, _: &mut Context<'_>, _: &mut [u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
}

/// A source over a blocking reader, such as the host process's stdin, read on a thread of its
/// own so no executor ever blocks on it. The thread starts on the first read and reads at most
/// one chunk ahead of the guest, so input nobody asked for stays with the host.
#[cfg(not(target_family = "wasm"))]
pub struct BlockingSource {
    shared: Arc<blocking::Shared>,
    reader: Mutex<Option<Box<dyn io::Read + Send>>>,
}

#[cfg(not(target_family = "wasm"))]
impl BlockingSource {
    pub fn new(reader: impl io::Read + Send + 'static) -> Self {
        Self {
            shared: Arc::default(),
            reader: Mutex::new(Some(Box::new(reader))),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl ByteSource for BlockingSource {
    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        let ready = self.shared.poll_read(cx, buf);
        if ready.is_pending()
            && let Some(reader) = self.reader.lock().unwrap().take()
        {
            let shared = self.shared.clone();
            let spawned = std::thread::Builder::new()
                .name("wasixv2-input".into())
                .spawn(move || shared.run(reader));
            if let Err(err) = spawned {
                return Poll::Ready(Err(err));
            }
        }
        ready
    }
}

#[cfg(not(target_family = "wasm"))]
impl Drop for BlockingSource {
    /// Lets the reader thread go once its current read returns.
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().closed = true;
        self.shared.wanted.notify_one();
    }
}

#[cfg(not(target_family = "wasm"))]
mod blocking {
    use std::{
        io,
        sync::{Condvar, Mutex},
        task::{Context, Poll, Waker},
    };

    /// How much the reader thread asks for at once.
    const CHUNK: usize = 16 * 1024;

    #[derive(Default)]
    pub(super) struct Shared {
        pub(super) state: Mutex<State>,
        /// Signalled when the consumer wants input, or the source is gone.
        pub(super) wanted: Condvar,
    }

    #[derive(Default)]
    pub(super) struct State {
        /// Input read but not consumed yet, from `pos` on.
        data: Vec<u8>,
        pos: usize,
        /// What the last host read ended with, if not data: end of stream or an error.
        end: Option<io::Result<()>>,
        want: bool,
        pub(super) closed: bool,
        waker: Option<Waker>,
    }

    impl Shared {
        pub(super) fn poll_read(
            &self,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let mut state = self.state.lock().unwrap();
            if state.pos < state.data.len() {
                let n = buf.len().min(state.data.len() - state.pos);
                buf[..n].copy_from_slice(&state.data[state.pos..][..n]);
                state.pos += n;
                return Poll::Ready(Ok(n));
            }
            if let Some(end) = state.end.take() {
                return Poll::Ready(end.map(|()| 0));
            }
            state.waker = Some(cx.waker().clone());
            if !state.want {
                state.want = true;
                self.wanted.notify_one();
            }
            Poll::Pending
        }

        /// The reader thread: one host read each time the consumer runs dry.
        pub(super) fn run(&self, mut reader: Box<dyn io::Read + Send>) {
            let mut chunk = vec![0; CHUNK];
            loop {
                {
                    let mut state = self.state.lock().unwrap();
                    while !state.want && !state.closed {
                        state = self.wanted.wait(state).unwrap();
                    }
                    if state.closed {
                        return;
                    }
                }
                let result = loop {
                    match reader.read(&mut chunk) {
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                        result => break result,
                    }
                };
                let waker = {
                    let mut state = self.state.lock().unwrap();
                    state.want = false;
                    match result {
                        Ok(0) => state.end = Some(Ok(())),
                        Ok(n) => {
                            state.data.clear();
                            state.data.extend_from_slice(&chunk[..n]);
                            state.pos = 0;
                        }
                        Err(err) => state.end = Some(Err(err)),
                    }
                    state.waker.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            }
        }
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

/// A captured host thread CPU clock, readable from another thread. Reads must stop before
/// the captured thread exits, since a host may reuse its clock identifier.
pub trait ThreadCpuClock: Send + Sync {
    /// CPU time used by the captured thread, excluding sleep and descheduling.
    fn read(&self) -> Option<Duration>;
}

/// Clocks, timers, entropy, shared memory and the executor that runs host work.
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
    /// Sleeps until a monotonic `deadline`, allowing expiry to be delayed by up to `slack`
    /// to share a wakeup with other timers. Never completes before `deadline`; executor and
    /// host scheduling can delay observation beyond the range. Dropping the future cancels it.
    ///
    /// Models Linux's `hrtimer_set_expires_range_ns` (`kernel/time/hrtimer.c`): `deadline`
    /// is the soft expiry, and `deadline + slack` the hard expiry. The default uses
    /// [`Host::sleep_until`], preserving hosts that do not coalesce timers.
    fn sleep_until_range(&self, deadline: Duration, slack: Duration) -> LocalFuture<'_, ()> {
        let _ = slack;
        self.sleep_until(deadline)
    }
    /// The CPU time the calling host thread has used so far, user and system time together
    /// (`CLOCK_THREAD_CPUTIME_ID`). Every guest thread runs on a host thread of its own, so the
    /// execution layer charges a guest thread with what this clock advances while it runs.
    /// `None` by default, and from hosts without such a clock, whose guests are charged with the
    /// wall-clock time they run instead.
    fn thread_cpu_time(&self) -> Option<Duration> {
        None
    }
    /// Captures the calling thread's CPU clock for periodic accounting on another thread.
    /// Hosts without cross-thread CPU clocks use boundary accounting instead.
    fn thread_cpu_clock(&self) -> Option<Arc<dyn ThreadCpuClock>> {
        None
    }
    /// Fills `buf` with cryptographically secure random bytes.
    fn random(&self, buf: &mut [u8]);
    /// Where host work runs: background futures and blocking calls.
    fn executor(&self) -> &dyn Executor;
    /// A message about a guest for whoever runs the host, such as why a process died of a
    /// trap; the kernel log (`dmesg`) of a Linux system. Goes to the host's stderr by default.
    fn report(&self, message: &str) {
        eprintln!("{message}");
    }

    /// A new shared memory object of `size` bytes, all zero, which processes map into their
    /// memories at once: what Linux's shmem gives `MAP_SHARED | MAP_ANONYMOUS` and SysV segments
    /// (`shmem_kernel_file_setup`). [`io::ErrorKind::Unsupported`] by default, and from hosts
    /// that have none, whose processes then keep copies of shared memory instead.
    fn shared_pages(&self, size: u64) -> io::Result<Arc<dyn SharedPages>> {
        let _ = size;
        Err(io::ErrorKind::Unsupported.into())
    }
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
    fn sleep_until_range_defaults_to_the_existing_deadline() {
        let host = SteppingHost::default();
        block_on(host.sleep_until_range(Duration::from_millis(10), Duration::from_millis(5)));
        assert_eq!(host.monotonic(), Duration::from_millis(10));
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
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            stdio.stdin.poll_read(&mut cx, &mut [0; 4]),
            Poll::Ready(Ok(0))
        ));
        assert_eq!(*written.lock().unwrap(), b"hi");
    }

    /// A blocking reader fed through a channel, like a terminal.
    #[cfg(not(target_family = "wasm"))]
    struct ChannelReader(std::sync::mpsc::Receiver<Vec<u8>>);

    #[cfg(not(target_family = "wasm"))]
    impl io::Read for ChannelReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let data = self.0.recv().unwrap_or_default();
            buf[..data.len()].copy_from_slice(&data);
            Ok(data.len())
        }
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn blocking_source_wakes_the_reader_when_input_arrives() {
        use std::task::{Wake, Waker};

        /// Counts wakes, so the test can wait for one.
        #[derive(Default)]
        struct Flag(std::sync::Mutex<bool>, std::sync::Condvar);

        impl Wake for Flag {
            fn wake(self: Arc<Self>) {
                *self.0.lock().unwrap() = true;
                self.1.notify_all();
            }
        }

        impl Flag {
            fn wait(&self) {
                let mut woken = self.0.lock().unwrap();
                while !*woken {
                    woken = self.1.wait(woken).unwrap();
                }
                *woken = false;
            }
        }

        let (send, recv) = std::sync::mpsc::channel();
        let source = BlockingSource::new(ChannelReader(recv));
        let flag = Arc::new(Flag::default());
        let waker = Waker::from(flag.clone());
        let mut cx = Context::from_waker(&waker);
        let mut buf = [0; 8];

        // Nothing yet: pending, without blocking the caller.
        assert!(source.poll_read(&mut cx, &mut buf).is_pending());
        send.send(b"abc".to_vec()).unwrap();
        flag.wait();
        assert!(matches!(
            source.poll_read(&mut cx, &mut buf),
            Poll::Ready(Ok(3))
        ));
        assert_eq!(&buf[..3], b"abc");

        // End of stream is reported once; the next read asks the host again.
        assert!(source.poll_read(&mut cx, &mut buf).is_pending());
        send.send(Vec::new()).unwrap();
        flag.wait();
        assert!(matches!(
            source.poll_read(&mut cx, &mut buf),
            Poll::Ready(Ok(0))
        ));
        assert!(source.poll_read(&mut cx, &mut buf).is_pending());
    }
}
