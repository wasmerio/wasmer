//! POSIX timers, on a host with real clocks and threads so the kernel's timer driver runs.

use std::time::Instant;

use wasixv2_abi::SigprocmaskHow;
use wasixv2_host::ThreadStart;

use super::{
    ipc::{errno, get, put},
    *,
};

const SIGUSR1: i32 = Signal::SIGUSR1.0;
const SIGALRM: i32 = Signal::SIGALRM.0;
const MS: Duration = Duration::from_millis(1);

const SIGEVENT: u32 = 0x100;
const TIMER_ID: u32 = 0x200;
const NEW: u32 = 0x300;
const OLD: u32 = 0x340;
const SET: u32 = 0x380;
const INFO: u32 = 0x400;
const TIMEOUT: u32 = 0x500;
const ACTION: u32 = 0x540;

struct RealHost(Instant);

impl Host for RealHost {
    fn realtime(&self) -> Duration {
        Duration::from_secs(1_700_000_000) + self.0.elapsed()
    }

    fn monotonic(&self) -> Duration {
        Duration::from_secs(100) + self.0.elapsed()
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

impl Executor for RealHost {
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
    kernel: Kernel,
    task: TaskId,
    memory: FlatMemory,
}

impl Fixture {
    fn new() -> Self {
        let host: Arc<dyn Host> = Arc::new(RealHost(Instant::now()));
        let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1);
        let kernel = Kernel::new(host, Arc::new(root));
        let task = kernel.spawn(ProcessConfig {
            args: vec![],
            env: vec![],
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        });
        let memory = FlatMemory(RefCell::new(vec![0; 65536]));
        Self {
            kernel,
            task,
            memory,
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    /// `timer_create` with `sigevent` (value, signo, notify, tid), or a null one.
    fn create(&self, clock: ClockId, event: Option<[i32; 4]>) -> SysResult<i32> {
        let spec = match event {
            Some(fields) => {
                let bytes: Vec<u8> = fields.iter().flat_map(|f| f.to_le_bytes()).collect();
                put(&self.memory, SIGEVENT, &bytes);
                GuestPtr::new(SIGEVENT)
            }
            None => GuestPtr::NULL,
        };
        let caller = self.caller();
        block_on(
            self.kernel
                .timer_create(&caller, clock, spec, GuestPtrMut::new(TIMER_ID)),
        )?;
        Ok(word(&self.memory, TIMER_ID) as i32)
    }

    fn signal_timer(&self, sig: i32, value: i32) -> i32 {
        let event = [value, sig, wasixv2_abi::SigevNotify::SIGEV_SIGNAL.0, 0];
        self.create(ClockId::CLOCK_MONOTONIC, Some(event)).unwrap()
    }

    /// `timer_settime`; returns the old setting as (value, interval) nanoseconds.
    fn settime(
        &self,
        id: i32,
        flags: TimerFlags,
        value: Duration,
        interval: Duration,
    ) -> SysResult<(u128, u128)> {
        let spec = [interval, value]
            .iter()
            .flat_map(|d| [d.as_secs() as i64, i64::from(d.subsec_nanos())])
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<u8>>();
        put(&self.memory, NEW, &spec);
        let caller = self.caller();
        block_on(self.kernel.timer_settime64(
            &caller,
            id,
            flags,
            GuestPtr::new(NEW),
            GuestPtrMut::new(OLD),
        ))?;
        let old = self.itimerspec(OLD);
        Ok((old.1, old.0))
    }

    fn arm(&self, id: i32, value: Duration, interval: Duration) {
        self.settime(id, TimerFlags(0), value, interval).unwrap();
    }

    /// (interval, value) in nanoseconds at `addr`.
    fn itimerspec(&self, addr: u32) -> (u128, u128) {
        let raw = get(&self.memory, addr, 32);
        let field = |i: usize| i64::from_le_bytes(raw[i * 8..][..8].try_into().unwrap()) as u128;
        (
            field(0) * 1_000_000_000 + field(1),
            field(2) * 1_000_000_000 + field(3),
        )
    }

    /// `timer_gettime` as (value, interval) nanoseconds.
    fn gettime(&self, id: i32) -> SysResult<(u128, u128)> {
        let caller = self.caller();
        block_on(
            self.kernel
                .timer_gettime64(&caller, id, GuestPtrMut::new(OLD)),
        )?;
        let (interval, value) = self.itimerspec(OLD);
        Ok((value, interval))
    }

    fn block(&self, sig: i32) {
        put(&self.memory, SET, &(1u64 << (sig - 1)).to_le_bytes());
        let caller = self.caller();
        let how = SigprocmaskHow::SIG_BLOCK;
        let set = GuestPtr::new(SET);
        block_on(
            self.kernel
                .rt_sigprocmask(&caller, how, set, GuestPtrMut::NULL, 8),
        )
        .unwrap();
    }

    fn sigaction(&self, sig: i32, handler: u32) {
        let mut action = [0; 16];
        action[0..4].copy_from_slice(&handler.to_le_bytes());
        put(&self.memory, ACTION, &action);
        let caller = self.caller();
        let act = GuestPtr::new(ACTION);
        block_on(
            self.kernel
                .rt_sigaction(&caller, Signal(sig), act, GuestPtrMut::NULL, 8),
        )
        .unwrap();
    }

    /// `sigtimedwait` for `sig`: (signo, code, timer id, overrun, value).
    fn wait(&self, sig: i32, timeout: Duration) -> SysResult<[i32; 5]> {
        put(&self.memory, SET, &(1u64 << (sig - 1)).to_le_bytes());
        let ts = [timeout.as_secs() as i64, i64::from(timeout.subsec_nanos())];
        put(&self.memory, TIMEOUT, &ts.map(i64::to_le_bytes).concat());
        let caller = self.caller();
        block_on(self.kernel.rt_sigtimedwait_time64(
            &caller,
            GuestPtr::new(SET),
            GuestPtrMut::new(INFO),
            GuestPtr::new(TIMEOUT),
            8,
        ))?;
        let field = |i: u32| word(&self.memory, INFO + i * 4) as i32;
        Ok([field(0), field(2), field(3), field(4), field(5)])
    }

    fn overrun(&self, id: i32) -> SysResult<i32> {
        block_on(self.kernel.timer_getoverrun(&self.caller(), id))
    }

    fn delete(&self, id: i32) -> SysResult<i32> {
        block_on(self.kernel.timer_delete(&self.caller(), id))
    }
}

const SI_TIMER: i32 = wasixv2_abi::SiCode::SI_TIMER.0;

#[test]
fn ids_count_up_per_process_and_are_not_reused_at_once() {
    let t = Fixture::new();
    assert_eq!(t.create(ClockId::CLOCK_MONOTONIC, None), Ok(0));
    assert_eq!(t.create(ClockId::CLOCK_REALTIME, None), Ok(1));
    assert_eq!(t.delete(0), Ok(0));
    assert_eq!(errno(t.delete(0)), Errno::EINVAL);
    assert_eq!(errno(t.gettime(0)), Errno::EINVAL);
    assert_eq!(t.create(ClockId::CLOCK_BOOTTIME, None), Ok(2));
    assert_eq!(errno(t.overrun(-1)), Errno::EINVAL);
    assert_eq!(errno(t.overrun(7)), Errno::EINVAL);
}

#[test]
fn create_checks_the_clock_and_the_sigevent_as_linux_does() {
    let t = Fixture::new();
    let sigev = |notify: i32, signo: i32, tid: i32| Some([0, signo, notify, tid]);
    let create = |clock, event| errno(t.create(clock, event));
    assert_eq!(create(ClockId(42), None), Errno::EINVAL);
    assert_eq!(
        create(ClockId::CLOCK_MONOTONIC_RAW, None),
        Errno::EOPNOTSUPP
    );
    assert_eq!(
        create(ClockId::CLOCK_REALTIME_COARSE, None),
        Errno::EOPNOTSUPP
    );
    assert_eq!(
        create(ClockId::CLOCK_MONOTONIC, sigev(3, SIGUSR1, 0)),
        Errno::EINVAL
    );
    assert_eq!(
        create(ClockId::CLOCK_MONOTONIC, sigev(0, 0, 0)),
        Errno::EINVAL
    );
    assert_eq!(
        create(ClockId::CLOCK_MONOTONIC, sigev(0, 65, 0)),
        Errno::EINVAL
    );
    // SIGEV_NONE takes any signal number.
    assert!(t.create(ClockId::CLOCK_MONOTONIC, sigev(1, 0, 0)).is_ok());
    // SIGEV_THREAD_ID needs a thread of the caller's group.
    assert_eq!(
        create(ClockId::CLOCK_MONOTONIC, sigev(4, SIGUSR1, 999)),
        Errno::EINVAL
    );
    let me = t.task.0 as i32;
    assert!(
        t.create(ClockId::CLOCK_MONOTONIC, sigev(4, SIGUSR1, me))
            .is_ok()
    );
    // The alarm clocks need CAP_WAKE_ALARM; the id is written out first.
    put(&t.memory, TIMER_ID, &[0xff; 4]);
    assert_eq!(create(ClockId::CLOCK_BOOTTIME_ALARM, None), Errno::EPERM);
    assert_ne!(word(&t.memory, TIMER_ID), u32::MAX);
    // CPU clocks of the caller.
    assert!(t.create(ClockId::CLOCK_PROCESS_CPUTIME_ID, None).is_ok());
    assert!(t.create(ClockId::CLOCK_THREAD_CPUTIME_ID, None).is_ok());
    // A dynamic (fd) clock has no timers.
    assert_eq!(create(ClockId((!3 << 3) | 3), None), Errno::EOPNOTSUPP);
}

#[test]
fn settime_checks_its_arguments() {
    let t = Fixture::new();
    let id = t.signal_timer(SIGUSR1, 0);
    let caller = t.caller();
    let null = block_on(t.kernel.timer_settime64(
        &caller,
        id,
        TimerFlags(0),
        GuestPtr::NULL,
        GuestPtrMut::NULL,
    ));
    assert_eq!(errno(null), Errno::EINVAL);
    put(&t.memory, NEW, &[0; 32]);
    put(&t.memory, NEW + 24, &1_000_000_000i64.to_le_bytes());
    let bad = block_on(t.kernel.timer_settime64(
        &caller,
        id,
        TimerFlags(0),
        GuestPtr::new(NEW),
        GuestPtrMut::NULL,
    ));
    assert_eq!(errno(bad), Errno::EINVAL);
    assert_eq!(
        errno(t.settime(id + 1, TimerFlags(0), MS, Duration::ZERO)),
        Errno::EINVAL
    );
}

#[test]
fn a_one_shot_timer_sends_one_si_timer_signal() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let id = t.signal_timer(SIGUSR1, 0x1234);
    t.arm(id, 20 * MS, Duration::ZERO);
    let (left, interval) = t.gettime(id).unwrap();
    assert!(left > 0 && left <= 20_000_000 && interval == 0);

    let info = t.wait(SIGUSR1, Duration::from_secs(5)).unwrap();
    assert_eq!(info, [SIGUSR1, SI_TIMER, id, 0, 0x1234]);
    assert_eq!(t.gettime(id), Ok((0, 0)));
    assert_eq!(errno(t.wait(SIGUSR1, 30 * MS)), Errno::EAGAIN);
    assert_eq!(t.overrun(id), Ok(0));
}

#[test]
fn the_default_sigevent_is_sigalrm_with_the_id_as_value() {
    let t = Fixture::new();
    t.block(SIGALRM);
    t.create(ClockId::CLOCK_MONOTONIC, None).unwrap();
    let id = t.create(ClockId::CLOCK_MONOTONIC, None).unwrap();
    t.arm(id, MS, Duration::ZERO);
    let info = t.wait(SIGALRM, Duration::from_secs(5)).unwrap();
    assert_eq!(info, [SIGALRM, SI_TIMER, id, 0, id]);
}

#[test]
fn periods_missed_while_the_signal_is_pending_become_the_overrun() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let id = t.signal_timer(SIGUSR1, 0);
    t.arm(id, 5 * MS, 5 * MS);
    std::thread::sleep(80 * MS);
    let [_, code, _, overrun, _] = t.wait(SIGUSR1, Duration::ZERO).unwrap();
    assert_eq!(code, SI_TIMER);
    // About 15 periods passed after the first expiry, all collapsed into one signal.
    assert!(overrun >= 10, "overrun {overrun}");
    assert_eq!(t.overrun(id), Ok(overrun));
    // Re-armed at delivery: the next signal comes with a small overrun.
    let [_, _, _, next, _] = t.wait(SIGUSR1, Duration::from_secs(5)).unwrap();
    assert!(next < overrun);
    let (left, interval) = t.gettime(id).unwrap();
    assert_eq!(interval, 5_000_000);
    assert!(left > 0 && left <= 5_000_000);
}

#[test]
fn deleting_or_re_arming_drops_a_pending_signal() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let deleted = t.signal_timer(SIGUSR1, 0);
    t.arm(deleted, MS, Duration::ZERO);
    std::thread::sleep(20 * MS);
    assert_eq!(t.delete(deleted), Ok(0));
    assert_eq!(errno(t.wait(SIGUSR1, Duration::ZERO)), Errno::EAGAIN);

    let rearmed = t.signal_timer(SIGUSR1, 0);
    t.arm(rearmed, MS, Duration::ZERO);
    std::thread::sleep(20 * MS);
    t.arm(rearmed, Duration::from_secs(60), Duration::ZERO);
    assert_eq!(errno(t.wait(SIGUSR1, Duration::ZERO)), Errno::EAGAIN);
    // Disarming with a zero value.
    let (left, _) = t
        .settime(rearmed, TimerFlags(0), Duration::ZERO, MS)
        .unwrap();
    assert!(left > 59_000_000_000);
    assert_eq!(t.gettime(rearmed), Ok((0, 0)));
}

#[test]
fn an_absolute_expiry_in_the_past_signals_at_once() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let event = [0, SIGUSR1, 0, 0];
    let id = t.create(ClockId::CLOCK_REALTIME, Some(event)).unwrap();
    let abstime = TimerFlags::TIMER_ABSTIME;
    t.settime(id, abstime, Duration::from_secs(1), Duration::ZERO)
        .unwrap();
    assert_eq!(t.wait(SIGUSR1, Duration::ZERO).unwrap()[1], SI_TIMER);

    // A future absolute expiry on the realtime clock.
    let now = Duration::from_secs(1_700_000_000) + Duration::from_secs(3600);
    t.settime(id, abstime, now, Duration::ZERO).unwrap();
    let (left, _) = t.gettime(id).unwrap();
    assert!(left > 3_000_000_000_000 && left <= 3_600_000_000_000);
}

#[test]
fn settime_returns_the_old_setting() {
    let t = Fixture::new();
    let id = t.signal_timer(SIGUSR1, 0);
    assert_eq!(
        t.settime(id, TimerFlags(0), Duration::from_secs(10), MS),
        Ok((0, 0))
    );
    let (left, interval) = t
        .settime(id, TimerFlags(0), Duration::from_secs(20), Duration::ZERO)
        .unwrap();
    assert!(left > 9_000_000_000 && left <= 10_000_000_000);
    assert_eq!(interval, 1_000_000);
}

#[test]
fn sigev_none_timers_count_down_without_signals() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let event = [0, SIGUSR1, wasixv2_abi::SigevNotify::SIGEV_NONE.0, 0];
    let once = t.create(ClockId::CLOCK_MONOTONIC, Some(event)).unwrap();
    let periodic = t.create(ClockId::CLOCK_MONOTONIC, Some(event)).unwrap();
    t.arm(once, 5 * MS, Duration::ZERO);
    t.arm(periodic, 5 * MS, 10 * MS);
    std::thread::sleep(30 * MS);
    assert_eq!(t.gettime(once), Ok((0, 0)));
    let (left, interval) = t.gettime(periodic).unwrap();
    assert_eq!(interval, 10_000_000);
    assert!(left > 0 && left <= 10_000_000);
    assert_eq!(errno(t.wait(SIGUSR1, Duration::ZERO)), Errno::EAGAIN);
}

#[test]
fn a_periodic_timer_signal_ignored_when_it_fired_is_queued_once_caught() {
    let t = Fixture::new();
    t.sigaction(SIGUSR1, wasixv2_abi::SignalHandler::SIG_IGN.0 as u32);
    let periodic = t.signal_timer(SIGUSR1, 1);
    let once = t.signal_timer(SIGUSR1, 2);
    t.arm(periodic, MS, 5 * MS);
    t.arm(once, MS, Duration::ZERO);
    std::thread::sleep(30 * MS);
    t.block(SIGUSR1);
    assert_eq!(errno(t.wait(SIGUSR1, Duration::ZERO)), Errno::EAGAIN);
    // Caught again: the periodic timer's signal comes back, the one-shot one is gone.
    t.sigaction(SIGUSR1, 0x1234);
    let [_, code, id, overrun, value] = t.wait(SIGUSR1, Duration::ZERO).unwrap();
    assert_eq!((code, id, value), (SI_TIMER, periodic, 1));
    assert!(overrun >= 3, "overrun {overrun}");
    t.delete(periodic).unwrap();
    assert_eq!(errno(t.wait(SIGUSR1, 20 * MS)), Errno::EAGAIN);
}

#[test]
fn a_thread_id_timer_signals_its_thread() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let me = t.task.0 as i32;
    let event = [7, SIGUSR1, wasixv2_abi::SigevNotify::SIGEV_THREAD_ID.0, me];
    let id = t.create(ClockId::CLOCK_MONOTONIC, Some(event)).unwrap();
    t.arm(id, MS, Duration::ZERO);
    assert_eq!(
        t.wait(SIGUSR1, Duration::from_secs(5)),
        Ok([SIGUSR1, SI_TIMER, id, 0, 7])
    );
}

#[test]
fn restore_ids_mode_takes_the_requested_id() {
    let t = Fixture::new();
    let prctl = |arg2, arg3| {
        let option = wasixv2_abi::PrctlOption::PR_TIMER_CREATE_RESTORE_IDS;
        block_on(t.kernel.prctl(&t.caller(), option, arg2, arg3, 0, 0))
    };
    assert_eq!(prctl(2, 0), Ok(0));
    assert_eq!(errno(prctl(1, 1)), Errno::EINVAL);
    assert_eq!(errno(prctl(3, 0)), Errno::EINVAL);
    assert_eq!(prctl(1, 0), Ok(0));
    assert_eq!(prctl(2, 0), Ok(1));
    put(&t.memory, TIMER_ID, &1000i32.to_le_bytes());
    assert_eq!(t.create(ClockId::CLOCK_MONOTONIC, None), Ok(1000));
    put(&t.memory, TIMER_ID, &1000i32.to_le_bytes());
    assert_eq!(
        errno(t.create(ClockId::CLOCK_MONOTONIC, None)),
        Errno::EBUSY
    );
    put(&t.memory, TIMER_ID, &(-1i32).to_le_bytes());
    assert_eq!(
        errno(t.create(ClockId::CLOCK_MONOTONIC, None)),
        Errno::EINVAL
    );
    assert_eq!(prctl(0, 0), Ok(0));
    assert_eq!(t.create(ClockId::CLOCK_MONOTONIC, None), Ok(1001));
}

#[test]
fn exit_deletes_every_timer() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let id = t.signal_timer(SIGUSR1, 0);
    t.arm(id, 10 * MS, Duration::ZERO);
    let group = t.kernel.task_signals(t.task).group().clone();
    crate::posix_timer::exit_itimers(&group);
    assert_eq!(errno(t.gettime(id)), Errno::EINVAL);
    assert_eq!(errno(t.wait(SIGUSR1, 40 * MS)), Errno::EAGAIN);
}

impl Fixture {
    /// A relative `clock_nanosleep` on CLOCK_MONOTONIC; returns its result and how long it took.
    fn nanosleep(&self, request: Duration) -> (SysResult<i32>, Duration) {
        let ts = [request.as_secs() as i64, i64::from(request.subsec_nanos())];
        put(&self.memory, TIMEOUT, &ts.map(i64::to_le_bytes).concat());
        let start = Instant::now();
        let caller = self.caller();
        let result = block_on(self.kernel.clock_nanosleep_time64(
            &caller,
            ClockId::CLOCK_MONOTONIC,
            TimerFlags(0),
            GuestPtr::new(TIMEOUT),
            GuestPtrMut::new(NEW),
        ));
        (result, start.elapsed())
    }
}

#[test]
fn a_blocked_timer_signal_does_not_cut_a_sleep_short() {
    let t = Fixture::new();
    t.block(SIGUSR1);
    let id = t.signal_timer(SIGUSR1, 0);
    t.arm(id, MS, MS);
    let (result, took) = t.nanosleep(50 * MS);
    assert_eq!(result, Ok(0));
    assert!(took >= 50 * MS, "slept {took:?}");
}

#[test]
fn a_caught_timer_signal_ends_a_sleep_with_the_time_left() {
    let t = Fixture::new();
    t.sigaction(SIGUSR1, 0x1234);
    let id = t.signal_timer(SIGUSR1, 0);
    t.arm(id, 10 * MS, Duration::ZERO);
    let (result, took) = t.nanosleep(Duration::from_secs(10));
    // The handler makes it EINTR; without one, `restart_syscall` would sleep the rest.
    assert_eq!(result, Err(SysErr::RestartBlock));
    assert!(took < Duration::from_secs(5));
    let left = i64::from_le_bytes(get(&t.memory, NEW, 8).try_into().unwrap());
    assert!((8..=10).contains(&left), "left {left} s");
}

/// `setitimer(ITIMER_REAL)` with (value, interval).
fn setitimer(t: &Fixture, value: Duration, interval: Duration) {
    let fields = [interval, value]
        .iter()
        .flat_map(|d| [d.as_secs() as i32, d.subsec_micros() as i32])
        .flat_map(i32::to_le_bytes)
        .collect::<Vec<u8>>();
    put(&t.memory, NEW, &fields);
    let caller = t.caller();
    let real = wasixv2_abi::ItimerWhich::ITIMER_REAL;
    let set = t
        .kernel
        .setitimer(&caller, real, GuestPtr::new(NEW), GuestPtrMut::NULL);
    assert_eq!(block_on(set), Ok(0));
}

/// `getitimer(ITIMER_REAL)` as (value, interval).
fn getitimer(t: &Fixture) -> (Duration, Duration) {
    let caller = t.caller();
    let real = wasixv2_abi::ItimerWhich::ITIMER_REAL;
    assert_eq!(
        block_on(t.kernel.getitimer(&caller, real, GuestPtrMut::new(OLD))),
        Ok(0)
    );
    let field = |i: u32| u64::from(word(&t.memory, OLD + i * 4));
    let timeval = |i: u32| Duration::from_secs(field(i)) + Duration::from_micros(field(i + 1));
    (timeval(2), timeval(0))
}

#[test]
fn a_periodic_itimer_goes_on_once_its_sigalrm_is_dequeued() {
    let t = Fixture::new();
    t.block(SIGALRM);
    setitimer(&t, 5 * MS, 5 * MS);
    std::thread::sleep(40 * MS);

    // Expired with its signal pending: not running until the signal is taken.
    assert_eq!(getitimer(&t), (Duration::ZERO, 5 * MS));
    assert_eq!(
        t.wait(SIGALRM, Duration::ZERO).map(|info| info[0]),
        Ok(SIGALRM)
    );
    let (left, interval) = getitimer(&t);
    assert!(!left.is_zero() && left <= 5 * MS, "{left:?}");
    assert_eq!(interval, 5 * MS);
    // The periods missed meanwhile were one signal.
    assert_eq!(errno(t.wait(SIGALRM, Duration::ZERO)), Errno::EAGAIN);

    // A zero value disarms the timer and forgets its interval.
    setitimer(&t, Duration::ZERO, 5 * MS);
    assert_eq!(getitimer(&t), (Duration::ZERO, Duration::ZERO));
}
