//! Observe timer ranges without wall-clock timing: Linux hrtimer_nanosleep,
//! fs/select.c select_estimate_accuracy, and futex_setup_timer.

use std::sync::atomic::{AtomicU64, Ordering};

use wasixv2_abi::{EpollCreateFlags, FutexOp, KernelTimespec, PrctlOption, PrioWhich};

use super::{ipc::errno, *};

#[derive(Default)]
struct RecordingHost {
    now: AtomicU64,
    ranges: Mutex<Vec<(Duration, Duration)>>,
}

impl Host for RecordingHost {
    fn realtime(&self) -> Duration {
        Duration::from_secs(1_700_000_000) + self.monotonic()
    }

    fn monotonic(&self) -> Duration {
        Duration::from_nanos(self.now.load(Ordering::Relaxed))
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        self.sleep_until(self.monotonic() + duration)
    }

    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        self.sleep_until_range(deadline, Duration::ZERO)
    }

    fn sleep_until_range(&self, deadline: Duration, slack: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async move {
            self.ranges.lock().unwrap().push((deadline, slack));
            self.now
                .fetch_max((deadline + slack).as_nanos() as u64, Ordering::Relaxed);
        })
    }

    fn random(&self, _: &mut [u8]) {}

    fn executor(&self) -> &dyn Executor {
        &TestHost
    }
}

struct Fixture {
    host: Arc<RecordingHost>,
    kernel: Kernel,
    task: TaskId,
    memory: FlatMemory,
}

impl Fixture {
    fn new() -> Self {
        let host = Arc::new(RecordingHost::default());
        let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 0, 0);
        let kernel =
            Kernel::new(host.clone(), Arc::new(root)).with_credentials(Credentials::new(0, 0));
        let (kernel, task, _) = spawn_on(kernel, Arc::new(Input(Mutex::new(vec![]))), &[], &[]);
        Self {
            host,
            kernel,
            task,
            memory: FlatMemory(RefCell::new(vec![0; 65536])),
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    fn slack(&self, value: u32) {
        assert_eq!(
            block_on(self.kernel.prctl(
                &self.caller(),
                PrctlOption::PR_SET_TIMERSLACK,
                value,
                0,
                0,
                0
            )),
            Ok(0)
        );
    }

    fn timeout(&self, duration: Duration) {
        block_on(wasixv2_syscalls::write_value(
            &self.memory,
            GuestPtrMut::new(0x1000),
            &time::to_timespec(duration),
        ))
        .unwrap();
    }

    fn last_slack(&self) -> Duration {
        self.host.ranges.lock().unwrap().last().unwrap().1
    }
}

#[test]
fn nanosleep_uses_the_callers_slack_for_relative_and_absolute_clocks() {
    let f = Fixture::new();
    let caller = f.caller();
    for slack in [1, 200_000, 0] {
        f.slack(slack);
        let expected = Duration::from_nanos(if slack == 0 { 50_000 } else { u64::from(slack) });
        for clock in [
            ClockId::CLOCK_MONOTONIC,
            ClockId::CLOCK_REALTIME,
            ClockId::CLOCK_TAI,
            ClockId::CLOCK_BOOTTIME,
        ] {
            for absolute in [false, true] {
                let duration = Duration::from_millis(1);
                let request = if absolute {
                    (match clock {
                        ClockId::CLOCK_REALTIME | ClockId::CLOCK_TAI => f.host.realtime(),
                        _ => f.host.monotonic(),
                    }) + duration
                } else {
                    duration
                };
                f.timeout(request);
                assert_eq!(
                    block_on(f.kernel.clock_nanosleep_time64(
                        &caller,
                        clock,
                        TimerFlags(if absolute { 1 } else { 0 }),
                        GuestPtr::new(0x1000),
                        GuestPtrMut::NULL
                    )),
                    Ok(0)
                );
                assert_eq!(
                    f.last_slack(),
                    expected,
                    "clock {clock:?}, absolute {absolute}"
                );
            }
        }
    }
}

#[test]
fn poll_and_epoll_use_linux_accuracy_and_rt_tasks_have_zero_slack() {
    let f = Fixture::new();
    let caller = f.caller();
    let epfd = block_on(f.kernel.epoll_create1(&caller, EpollCreateFlags(0))).unwrap();
    for (nice, slack_ns, expected) in [
        (0, 1, 1_000_000),
        (5, 1, 5_000_000),
        (5, 20_000_000, 20_000_000),
    ] {
        assert_eq!(
            block_on(
                f.kernel
                    .setpriority(&caller, PrioWhich::PRIO_PROCESS, 0, nice)
            ),
            Ok(0)
        );
        f.slack(slack_ns);
        f.timeout(Duration::from_secs(1));
        assert_eq!(
            block_on(f.kernel.ppoll_time64(
                &caller,
                GuestPtrMut::NULL,
                0,
                GuestPtrMut::new(0x1000),
                GuestPtr::NULL,
                0
            )),
            Ok(0)
        );
        assert_eq!(f.last_slack(), Duration::from_nanos(expected));
        f.timeout(Duration::from_secs(1));
        assert_eq!(
            block_on(f.kernel.epoll_pwait2(
                &caller,
                epfd,
                GuestPtrMut::new(0x2000),
                1,
                GuestPtr::new(0x1000),
                GuestPtr::NULL,
                0
            )),
            Ok(0)
        );
        assert_eq!(f.last_slack(), Duration::from_nanos(expected));
    }
    block_on(wasixv2_syscalls::write_value(
        &f.memory,
        GuestPtrMut::new(0x3000),
        &wasixv2_abi::SchedParam { sched_priority: 1 },
    ))
    .unwrap();
    assert_eq!(
        block_on(f.kernel.sched_setscheduler(
            &caller,
            0,
            SchedPolicy::SCHED_FIFO,
            GuestPtr::new(0x3000)
        )),
        Ok(0)
    );
    f.timeout(Duration::from_secs(1));
    assert_eq!(
        block_on(f.kernel.ppoll_time64(
            &caller,
            GuestPtrMut::NULL,
            0,
            GuestPtrMut::new(0x1000),
            GuestPtr::NULL,
            0
        )),
        Ok(0)
    );
    assert_eq!(f.last_slack(), Duration::ZERO);
    f.timeout(Duration::from_millis(1));
    assert_eq!(
        block_on(f.kernel.clock_nanosleep_time64(
            &caller,
            ClockId::CLOCK_MONOTONIC,
            TimerFlags(0),
            GuestPtr::new(0x1000),
            GuestPtrMut::NULL
        )),
        Ok(0)
    );
    assert_eq!(f.last_slack(), Duration::ZERO);
}

#[test]
fn ordinary_futex_waits_use_task_slack_on_both_clocks() {
    let f = Fixture::new();
    let caller = f.caller();
    f.slack(200_000);
    for realtime in [false, true] {
        let clock = if realtime {
            f.host.realtime()
        } else {
            f.host.monotonic()
        };
        f.timeout(clock + Duration::from_millis(1));
        let op = FutexOp(
            FutexOp::FUTEX_WAIT_BITSET.0
                | FutexOp::FUTEX_PRIVATE_FLAG.0
                | if realtime {
                    FutexOp::FUTEX_CLOCK_REALTIME.0
                } else {
                    0
                },
        );
        let result = block_on(f.kernel.futex_time64(
            &caller,
            GuestPtrMut::new(0x2000),
            op,
            0,
            0x1000,
            GuestPtrMut::NULL,
            u32::MAX,
        ));
        assert_eq!(errno(result), Errno::ETIMEDOUT);
        assert_eq!(f.last_slack(), Duration::from_micros(200));
    }
}

#[test]
fn interrupted_nanosleep_restarts_at_the_hard_expiry_without_adding_slack() {
    let f = Fixture::new();
    let caller = f.caller();
    f.slack(200_000);
    f.timeout(Duration::from_secs(1));
    f.kernel.kill(f.task, Signal::SIGUSR1).unwrap();
    assert_eq!(
        block_on(f.kernel.clock_nanosleep_time64(
            &caller,
            ClockId::CLOCK_MONOTONIC,
            TimerFlags(0),
            GuestPtr::new(0x1000),
            GuestPtrMut::new(0x2000)
        )),
        Err(SysErr::RestartBlock)
    );
    let left: KernelTimespec = block_on(wasixv2_syscalls::read_value(
        &f.memory,
        GuestPtr::new(0x2000),
    ))
    .unwrap();
    assert_eq!(
        time::from_timespec(left).unwrap(),
        Duration::from_micros(1_000_200)
    );
    f.kernel.task_signals(f.task).replace_blocked(1 << 9);
    assert_eq!(block_on(f.kernel.restart_syscall(&caller)), Ok(0));
    let ranges = f.host.ranges.lock().unwrap();
    assert_eq!(
        ranges.last(),
        Some(&(Duration::from_micros(1_000_200), Duration::ZERO))
    );
}

#[test]
fn alarm_and_futex2_waits_do_not_inherit_nanosleep_slack() {
    let f = Fixture::new();
    let caller = f.caller();
    f.slack(20_000_000);
    for clock in [ClockId::CLOCK_REALTIME_ALARM, ClockId::CLOCK_BOOTTIME_ALARM] {
        f.timeout(Duration::from_millis(1));
        assert_eq!(
            block_on(f.kernel.clock_nanosleep_time64(
                &caller,
                clock,
                TimerFlags(0),
                GuestPtr::new(0x1000),
                GuestPtrMut::NULL
            )),
            Ok(0)
        );
        assert_eq!(f.last_slack(), Duration::ZERO);
    }
    f.timeout(f.host.monotonic() + Duration::from_millis(1));
    let result = block_on(f.kernel.futex_wait(
        &caller,
        GuestPtrMut::new(0x2000),
        0,
        u32::MAX,
        wasixv2_abi::Futex2Flags::FUTEX_32,
        GuestPtrMut::new(0x1000),
        ClockId::CLOCK_MONOTONIC,
    ));
    assert_eq!(errno(result), Errno::ETIMEDOUT);
    assert_eq!(f.last_slack(), Duration::ZERO);
}
