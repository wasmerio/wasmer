//! Timer descriptors and sets of the realtime clock (`TFD_TIMER_CANCEL_ON_SET`), on a host
//! with real clocks and threads, so the kernel's timer driver runs, whose wall clock a test
//! can set.

use std::{
    sync::atomic::{AtomicI64, Ordering},
    time::Instant,
};

use wasixv2_abi::{KernelItimerspec, TimerfdFlags, TimerfdSettimeFlags};
use wasixv2_host::ThreadStart;
use wasixv2_syscalls::{read_value, write_value};

use super::*;

const SPEC: u32 = 0x100;
const OLD: u32 = 0x140;
const BUF: u32 = 0x180;
const ABSTIME: TimerfdSettimeFlags = TimerfdSettimeFlags::TFD_TIMER_ABSTIME;
const CANCEL_ON_SET: TimerfdSettimeFlags = TimerfdSettimeFlags(
    TimerfdSettimeFlags::TFD_TIMER_ABSTIME.0 | TimerfdSettimeFlags::TFD_TIMER_CANCEL_ON_SET.0,
);

/// Real clocks, with a wall clock that [`StepHost::set`] moves.
struct StepHost {
    start: Instant,
    /// Nanoseconds the wall clock was set by.
    step: AtomicI64,
}

impl StepHost {
    fn set(&self, by: Duration, forward: bool) {
        let by = by.as_nanos() as i64;
        self.step
            .fetch_add(if forward { by } else { -by }, Ordering::SeqCst);
    }
}

impl Host for StepHost {
    fn realtime(&self) -> Duration {
        let real = Duration::from_secs(1_700_000_000) + self.start.elapsed();
        let step = self.step.load(Ordering::SeqCst);
        match step >= 0 {
            true => real + Duration::from_nanos(step as u64),
            false => real - Duration::from_nanos(step.unsigned_abs()),
        }
    }

    fn monotonic(&self) -> Duration {
        Duration::from_secs(100) + self.start.elapsed()
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        let (send, done) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            std::thread::sleep(duration);
            let _ = send.send(());
        });
        Box::pin(async move {
            let _ = done.await;
        })
    }

    fn random(&self, buf: &mut [u8]) {
        buf.fill(0x5a);
    }

    fn executor(&self) -> &dyn Executor {
        self
    }
}

impl Executor for StepHost {
    fn spawn_local(&self, _: LocalFuture<'static, ()>) {
        unreachable!("the timer driver gets a thread")
    }

    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        Box::pin(async move { work() })
    }

    fn spawn_thread(&self, start: ThreadStart) -> io::Result<()> {
        std::thread::spawn(move || block_on(start()));
        Ok(())
    }
}

struct Fixture {
    host: Arc<StepHost>,
    kernel: Kernel,
    task: TaskId,
    memory: FlatMemory,
}

impl Fixture {
    fn new() -> Self {
        let host = Arc::new(StepHost {
            start: Instant::now(),
            step: AtomicI64::new(0),
        });
        let dyn_host: Arc<dyn Host> = host.clone();
        let root = MemFs::new(Arc::new(HostClock(dyn_host.clone())), 0o755, 1, 1);
        let kernel = Kernel::new(dyn_host, Arc::new(root));
        let task = kernel.spawn(ProcessConfig {
            args: vec![],
            env: vec![],
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        });
        let memory = FlatMemory(RefCell::new(vec![0; 4096]));
        Self {
            host,
            kernel,
            task,
            memory,
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    /// A non-blocking timer descriptor on `clock`.
    fn create(&self, clock: ClockId) -> Fd {
        self.create_with(clock, TimerfdFlags::TFD_NONBLOCK)
    }

    fn create_with(&self, clock: ClockId, flags: TimerfdFlags) -> Fd {
        block_on(self.kernel.timerfd_create(&self.caller(), clock, flags)).unwrap()
    }

    /// A blocking read of `fd`, given up after `limit`: the expirations, or `None` if nothing
    /// woke the reader by then.
    fn blocking_read(&self, fd: Fd, limit: Duration) -> Option<SysResult<u32>> {
        let caller = self.caller();
        let read = self.kernel.read(&caller, fd, UserSliceMut::new(BUF, 8));
        let timeout = self.host.sleep(limit);
        match block_on(futures::future::select(Box::pin(read), timeout)) {
            futures::future::Either::Left((done, _)) => Some(done),
            futures::future::Either::Right(_) => None,
        }
    }

    /// `timerfd_settime` for a one-shot `value`.
    fn settime(&self, fd: Fd, flags: TimerfdSettimeFlags, value: Duration) -> SysResult<i32> {
        let spec = KernelItimerspec {
            it_interval: time::to_timespec(Duration::ZERO),
            it_value: time::to_timespec(value),
        };
        let caller = self.caller();
        block_on(write_value(&self.memory, GuestPtrMut::new(SPEC), &spec)).unwrap();
        let (new, old) = (GuestPtr::new(SPEC), GuestPtrMut::new(OLD));
        block_on(self.kernel.timerfd_settime64(&caller, fd, flags, new, old))
    }

    /// What is left of `fd`'s setting.
    fn left(&self, fd: Fd) -> Duration {
        let caller = self.caller();
        let got = self
            .kernel
            .timerfd_gettime64(&caller, fd, GuestPtrMut::new(OLD));
        assert_eq!(block_on(got), Ok(0));
        let spec: KernelItimerspec =
            block_on(read_value(&self.memory, GuestPtr::new(OLD))).unwrap();
        time::from_timespec(spec.it_value).unwrap()
    }

    /// A non-blocking read: the expirations, or the error.
    fn read(&self, fd: Fd) -> Result<u64, Errno> {
        let caller = self.caller();
        match block_on(self.kernel.read(&caller, fd, UserSliceMut::new(BUF, 8))) {
            Ok(8) => Ok(u64::from_ne_bytes(
                self.memory.0.borrow()[BUF as usize..][..8]
                    .try_into()
                    .unwrap(),
            )),
            Ok(n) => panic!("a timerfd read of {n} bytes"),
            Err(SysErr::Errno(errno)) => Err(errno),
            Err(err) => panic!("a non-blocking read failed with {err:?}"),
        }
    }

    /// The wall clock's time `after` from now.
    fn wall_in(&self, after: Duration) -> Duration {
        self.host.realtime() + after
    }
}

const HOUR: Duration = Duration::from_secs(3600);

/// A set of the realtime clock cancels an absolute realtime timer with
/// `TFD_TIMER_CANCEL_ON_SET`: its next read fails with `ECANCELED`, once, and the timer stays
/// armed. Without the flag, or with a relative setting, a set changes nothing but the deadline.
#[test]
fn a_set_of_the_clock_cancels_an_absolute_realtime_timer_that_asks_for_it() {
    let fx = Fixture::new();
    let canceling = fx.create(ClockId::CLOCK_REALTIME);
    let absolute = fx.create(ClockId::CLOCK_REALTIME);
    let relative = fx.create(ClockId::CLOCK_REALTIME);
    assert_eq!(
        fx.settime(canceling, CANCEL_ON_SET, fx.wall_in(HOUR)),
        Ok(0)
    );
    assert_eq!(fx.settime(absolute, ABSTIME, fx.wall_in(HOUR)), Ok(0));
    let relative_cancel = TimerfdSettimeFlags::TFD_TIMER_CANCEL_ON_SET;
    assert_eq!(fx.settime(relative, relative_cancel, HOUR), Ok(0));
    assert_eq!(fx.read(canceling), Err(Errno::EAGAIN), "no set yet");

    fx.host.set(Duration::from_secs(10), false);

    assert_eq!(fx.read(canceling), Err(Errno::ECANCELED));
    assert_eq!(fx.read(canceling), Err(Errno::EAGAIN), "reported once");
    assert!(
        fx.left(canceling) > HOUR - Duration::from_secs(1),
        "still armed"
    );
    assert_eq!(fx.read(absolute), Err(Errno::EAGAIN));
    assert_eq!(fx.read(relative), Err(Errno::EAGAIN));
}

/// A set seen when a canceling timer fires also re-derives the monotonic deadlines of the
/// machine's other realtime timers (`clock_was_set`), so a reader blocked on one whose deadline
/// the clock jumped past wakes then, not when its old monotonic deadline comes.
#[test]
fn a_set_seen_at_an_expiry_fires_the_realtime_timers_it_jumped_past() {
    let fx = Fixture::new();
    let canceling = fx.create(ClockId::CLOCK_REALTIME);
    let later = fx.create_with(ClockId::CLOCK_REALTIME, TimerfdFlags(0));
    let monotonic = fx.create(ClockId::CLOCK_MONOTONIC);
    let soon = Duration::from_millis(50);
    assert_eq!(
        fx.settime(canceling, CANCEL_ON_SET, fx.wall_in(soon)),
        Ok(0)
    );
    assert_eq!(fx.settime(later, ABSTIME, fx.wall_in(HOUR)), Ok(0));
    assert_eq!(fx.settime(monotonic, TimerfdSettimeFlags(0), HOUR), Ok(0));

    // The clock jumps past `later`'s deadline while a reader is blocked on it, and nothing
    // looks at `later` until the canceling timer fires, 50 ms on.
    let host = fx.host.clone();
    let setter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        host.set(2 * HOUR, true);
    });
    let woken = fx.blocking_read(later, Duration::from_secs(5));
    assert_eq!(woken, Some(Ok(8)), "woken by the re-derived deadline");
    setter.join().unwrap();
    assert_eq!(fx.read(canceling), Err(Errno::ECANCELED));
    assert_eq!(fx.read(monotonic), Err(Errno::EAGAIN));
}

/// Arming a canceling timer again after an unreported set arms it and fails with
/// `ECANCELED` (`timerfd_setup`), which reports the set.
#[test]
fn rearming_after_an_unreported_set_fails_with_ecanceled_and_arms() {
    let fx = Fixture::new();
    let fd = fx.create(ClockId::CLOCK_REALTIME);
    assert_eq!(fx.settime(fd, CANCEL_ON_SET, fx.wall_in(HOUR)), Ok(0));

    fx.host.set(Duration::from_secs(10), true);

    let rearm = fx.settime(fd, CANCEL_ON_SET, fx.wall_in(2 * HOUR));
    assert_eq!(rearm, Err(SysErr::Errno(Errno::ECANCELED)));
    assert!(fx.left(fd) > HOUR, "the new setting stands");
    assert_eq!(fx.read(fd), Err(Errno::EAGAIN), "the set was reported");
    assert_eq!(fx.settime(fd, CANCEL_ON_SET, fx.wall_in(HOUR)), Ok(0));
}

/// A set before the timer cancels on sets is not one it is canceled for.
#[test]
fn a_set_before_cancel_on_set_is_asked_for_does_not_cancel() {
    let fx = Fixture::new();
    let fd = fx.create(ClockId::CLOCK_REALTIME);
    fx.host.set(Duration::from_secs(10), true);
    assert_eq!(fx.settime(fd, CANCEL_ON_SET, fx.wall_in(HOUR)), Ok(0));
    assert_eq!(fx.read(fd), Err(Errno::EAGAIN));
}
