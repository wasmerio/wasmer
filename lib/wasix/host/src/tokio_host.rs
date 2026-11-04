use std::{
    io,
    sync::{Arc, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    runtime::{Handle, RuntimeFlavor},
    sync::Mutex,
    task::LocalSet,
};

use crate::{BlockingSource, ByteSink, Executor, Host, LocalFuture, Stdio, ThreadStart};

/// The host on a tokio runtime: system clocks, native deadline timers, the OS entropy
/// source, and tokio as the executor.
///
/// [`Executor::spawn_local`] uses `tokio::task::spawn_local`, so the kernel must be driven inside
/// a `tokio::task::LocalSet` (for example `LocalSet::block_on`). Blocking work runs on tokio's
/// blocking pool. Each guest thread gets its own OS thread with a `LocalSet` of its own, since
/// the futures that run guests are not `Send`, driven on the runtime of the thread that started
/// it when that is a multi-thread runtime, whose workers drive I/O for every
/// thread, so all guest threads share one runtime, its I/O driver and blocking pool. A
/// current-thread runtime drives them only while its own thread waits in it, so on one (or
/// outside any runtime) a guest thread gets a current-thread runtime of its own.
pub struct TokioHost {
    // Native timers and the monotonic clock use the same real clock.
    start: Instant,
    timers: OnceLock<crate::timer::Timers>,
}

impl Default for TokioHost {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            timers: OnceLock::new(),
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
        self.sleep_until(self.monotonic().saturating_add(duration))
    }

    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        self.sleep_until_range(deadline, Duration::ZERO)
    }

    fn sleep_until_range(&self, deadline: Duration, slack: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async move {
            // A deadline too far out to represent never arrives.
            match self.start.checked_add(deadline) {
                Some(deadline) => {
                    self.timers
                        .get_or_init(crate::timer::Timers::new)
                        .sleep(deadline, slack)
                        .await
                }
                None => std::future::pending().await,
            }
        })
    }

    #[cfg(unix)]
    fn thread_cpu_time(&self) -> Option<Duration> {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `ts` is a valid timespec to write, and the clock id is a constant.
        let ret = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
        (ret == 0).then(|| Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn thread_cpu_clock(&self) -> Option<Arc<dyn crate::ThreadCpuClock>> {
        let mut id = 0;
        // SAFETY: pthread_self names this live thread and id is writable. Only the captured
        // clock id crosses threads; no pthread_t is dereferenced after capture.
        let ret = unsafe { libc::pthread_getcpuclockid(libc::pthread_self(), &mut id) };
        (ret == 0).then(|| Arc::new(CpuClock(id)) as Arc<dyn crate::ThreadCpuClock>)
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

#[cfg(any(target_os = "linux", target_os = "android"))]
struct CpuClock(libc::clockid_t);

#[cfg(any(target_os = "linux", target_os = "android"))]
impl crate::ThreadCpuClock for CpuClock {
    fn read(&self) -> Option<Duration> {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: this is a CPU clock id captured while its owning thread was alive, and ts
        // is writable. The caller stops reading before that thread exits.
        let ret = unsafe { libc::clock_gettime(self.0, &mut ts) };
        (ret == 0).then(|| Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))
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

    #[tokio::test]
    async fn sleep_until_waits_for_the_monotonic_deadline() {
        let host = TokioHost::default();
        let deadline = host.monotonic() + Duration::from_millis(3);
        host.sleep_until(deadline).await;
        assert!(host.monotonic() >= deadline);
    }

    /// LTP tst_timer_test permits 400 us plus the default 50 us task slack.
    #[tokio::test]
    async fn sleeps_do_not_add_a_millisecond_tick() {
        let host = TokioHost::default();
        let mut overshoot = Vec::new();
        for _ in 0..64 {
            let deadline = host.monotonic() + Duration::from_millis(1);
            host.sleep_until(deadline).await;
            let now = host.monotonic();
            assert!(now >= deadline);
            overshoot.push(now - deadline);
        }
        overshoot.sort();
        assert!(
            overshoot[32] < Duration::from_micros(450),
            "median overshoot: {:?}",
            overshoot[32]
        );
    }

    /// Run explicitly to compare precision, total process CPU and idle context switches.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "timer measurement, run alone with --nocapture"]
    async fn measure_timers() {
        fn usage() -> libc::rusage {
            let mut usage = std::mem::MaybeUninit::uninit();
            // SAFETY: getrusage writes the entire object on success.
            assert_eq!(
                unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
                0
            );
            unsafe { usage.assume_init() }
        }
        fn cpu(usage: &libc::rusage) -> f64 {
            let seconds = usage.ru_utime.tv_sec + usage.ru_stime.tv_sec;
            let micros = usage.ru_utime.tv_usec + usage.ru_stime.tv_usec;
            seconds as f64 + micros as f64 / 1e6
        }
        let host = TokioHost::default();
        for us in [100, 1000, 10000] {
            let before = usage();
            let start = std::time::Instant::now();
            let mut overshoot = Vec::new();
            for _ in 0..200 {
                let deadline = host.monotonic() + Duration::from_micros(us);
                host.sleep_until(deadline).await;
                overshoot.push(host.monotonic().saturating_sub(deadline).as_nanos());
            }
            let after = usage();
            overshoot.sort();
            eprintln!(
                "sleep_us={us} n=200 overshoot_ns p50={} p95={} max={} wall_s={:.6} cpu_s={:.6} voluntary_cs={} involuntary_cs={}",
                overshoot[100],
                overshoot[190],
                overshoot[199],
                start.elapsed().as_secs_f64(),
                cpu(&after) - cpu(&before),
                after.ru_nvcsw - before.ru_nvcsw,
                after.ru_nivcsw - before.ru_nivcsw
            );
        }
        let before = usage();
        std::thread::sleep(Duration::from_secs(2));
        let after = usage();
        eprintln!(
            "idle_s=2 cpu_s={:.6} voluntary_cs={} involuntary_cs={}",
            cpu(&after) - cpu(&before),
            after.ru_nvcsw - before.ru_nvcsw,
            after.ru_nivcsw - before.ru_nivcsw
        );
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

    /// Another thread's clock excludes the sampler's CPU and advances when its owner runs.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn captured_cpu_clock_reads_the_target_thread_not_the_sampler() {
        use std::sync::mpsc;
        let (send_clock, recv_clock) = mpsc::channel();
        let (send_cpu, recv_cpu) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let host = TokioHost::default();
            send_clock.send(host.thread_cpu_clock().unwrap()).unwrap();
            wait.recv().unwrap();
            let start = host.thread_cpu_time().unwrap();
            while host.thread_cpu_time().unwrap() - start < Duration::from_millis(5) {
                std::hint::spin_loop();
            }
            send_cpu.send(host.thread_cpu_time().unwrap()).unwrap();
            wait.recv().unwrap();
        });
        let clock = recv_clock.recv().unwrap();
        let before = clock.read().unwrap();
        let main = TokioHost::default();
        let start = main.thread_cpu_time().unwrap();
        while main.thread_cpu_time().unwrap() - start < Duration::from_millis(30) {
            std::hint::spin_loop();
        }
        assert!(clock.read().unwrap() - before < Duration::from_millis(10));
        release.send(()).unwrap();
        let after = recv_cpu.recv().unwrap();
        assert!(after - before >= Duration::from_millis(5));
        assert!(clock.read().unwrap() >= after);
        // Stop reading before allowing the target thread's clock id to be reused.
        drop(clock);
        release.send(()).unwrap();
        worker.join().unwrap();
    }

    /// The thread CPU clock advances while the thread computes, and hardly while it sleeps.
    #[cfg(unix)]
    #[test]
    fn thread_cpu_time_counts_computing_not_sleeping() {
        let host = TokioHost::default();
        let start = host.thread_cpu_time().unwrap();
        let wall = std::time::Instant::now();
        let mut x = 0u64;
        while wall.elapsed() < Duration::from_millis(30) {
            x = std::hint::black_box(x.wrapping_mul(31).wrapping_add(7));
        }
        let computed = host.thread_cpu_time().unwrap() - start;
        assert!(computed >= Duration::from_millis(10), "{computed:?}");
        std::thread::sleep(Duration::from_millis(50));
        let slept = host.thread_cpu_time().unwrap() - start - computed;
        assert!(slept < Duration::from_millis(10), "{slept:?}");
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
