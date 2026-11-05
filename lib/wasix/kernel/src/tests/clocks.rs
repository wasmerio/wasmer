//! The machine's clocks: a wall clock the machine owns (`clock_settime`, `clock_adjtime`), the
//! alarm clocks, and time namespaces.

use wasixv2_abi::{
    AdjtimexModes, ClockState, CloneFlags, KernelItimerspec, KernelTimespec, KernelTimex, Sigevent,
    Signal, Statx, StatxMask, TimerFlags, TimerfdFlags, TimexStatus,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;

use super::{ipc::errno, proc::ForkableMemory, *};

const CWD: Fd = Fd(-100);
const SIGCHLD: u32 = Signal::SIGCHLD.0 as u32;
const TS: u32 = 0x100;
const TIMEX: u32 = 0x200;
const TIMER_ID: u32 = 0x400;
const EVENT: u32 = 0x440;
const SPEC: u32 = 0x500;
const PATH: u32 = 0x1000;
const STATX: u32 = 0x3000;
const BUF: u32 = 0x5000;
/// What [`TestHost`] reads: the wall clock and the monotonic clock.
const HOST_REAL: Duration = Duration::from_secs(1_700_000_000);
const HOST_MONO: Duration = Duration::from_millis(1500);

/// A process of the test: its first task and its memory.
struct P {
    task: TaskId,
    memory: ForkableMemory,
}

impl P {
    fn new(task: TaskId) -> Self {
        Self {
            task,
            memory: ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536]))),
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }
}

/// A machine as the api builds one: its wall clock is a [`Timekeeper`] over the host's, which
/// its root filesystem stamps files with. One process runs as `uid`; `/proc` is mounted.
fn machine_as(uid: u32) -> (Kernel, P) {
    let time = Timekeeper::new(Arc::new(TestHost));
    let root = MemFs::new(Arc::new(HostClock(time.clone())), 0o777, 0, 0);
    let kernel = Kernel::with_timekeeper(time, Arc::new(root))
        .with_credentials(Credentials::new(uid, uid))
        .with_init();
    let task = kernel
        .spawn(ProcessConfig {
            args: vec!["test".into()],
            env: Vec::new(),
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        })
        .unwrap();
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

fn ts(time: Duration) -> KernelTimespec {
    time::to_timespec(time)
}

fn now(kernel: &Kernel, p: &P, clock: ClockId) -> SysResult<Duration> {
    let out = GuestPtrMut::new(TS);
    block_on(kernel.clock_gettime64(&p.caller(), clock, out))?;
    let ts = block_on(read_value(&p.memory, GuestPtr::<KernelTimespec>::new(TS))).unwrap();
    Ok(time::from_timespec(ts).unwrap())
}

fn settime(kernel: &Kernel, p: &P, clock: ClockId, to: KernelTimespec) -> SysResult<i32> {
    block_on(write_value(&p.memory, GuestPtrMut::new(TS), &to)).unwrap();
    block_on(kernel.clock_settime64(&p.caller(), clock, GuestPtr::new(TS)))
}

/// `clock_adjtime` with `txc`: the state and what came back.
fn adjtime(
    kernel: &Kernel,
    p: &P,
    clock: ClockId,
    txc: KernelTimex,
) -> SysResult<(i32, KernelTimex)> {
    block_on(write_value(&p.memory, GuestPtrMut::new(TIMEX), &txc)).unwrap();
    let state = block_on(kernel.clock_adjtime64(&p.caller(), clock, GuestPtrMut::new(TIMEX)))?;
    let txc = block_on(read_value(&p.memory, GuestPtr::<KernelTimex>::new(TIMEX))).unwrap();
    Ok((state, txc))
}

fn txc(modes: AdjtimexModes) -> KernelTimex {
    KernelTimex {
        modes,
        ..KernelTimex::default()
    }
}

fn path(p: &P, text: &str) -> Cstr {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    block_on(p.memory.write(PATH, &bytes)).unwrap();
    GuestPtr::new(PATH)
}

fn open(kernel: &Kernel, p: &P, at: &str, flags: OpenFlags) -> SysResult<Fd> {
    block_on(kernel.openat(&p.caller(), CWD, path(p, at), flags, 0o644))
}

fn read_text(kernel: &Kernel, p: &P, at: &str) -> String {
    let caller = p.caller();
    let fd = open(kernel, p, at, OpenFlags::O_RDONLY).unwrap();
    let n = block_on(kernel.read(&caller, fd, UserSliceMut::new(BUF, 4096))).unwrap();
    let mut out = vec![0; n as usize];
    block_on(p.memory.read(BUF, &mut out)).unwrap();
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    String::from_utf8(out).unwrap()
}

fn write_text(kernel: &Kernel, p: &P, at: &str, text: &str) -> SysResult<u32> {
    let caller = p.caller();
    let fd = open(kernel, p, at, OpenFlags::O_WRONLY)?;
    block_on(p.memory.write(BUF, text.as_bytes())).unwrap();
    let written = block_on(kernel.write(&caller, fd, UserSlice::new(BUF, text.len() as u32)));
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    written
}

fn mtime(kernel: &Kernel, p: &P, at: &str) -> i64 {
    let out = GuestPtrMut::new(STATX);
    let caller = p.caller();
    let call = kernel.statx(&caller, CWD, path(p, at), AtFlags(0), StatxMask(0x7ff), out);
    block_on(call).unwrap();
    let statx: Statx = block_on(read_value(&p.memory, GuestPtr::new(STATX))).unwrap();
    statx.stx_mtime.tv_sec
}

fn fork(kernel: &Kernel, parent: &P) -> P {
    let null = GuestPtrMut::NULL;
    block_on(kernel.clone(&parent.caller(), SIGCHLD, 0, null, 0, null)).unwrap();
    let mut child = None;
    while let Some(action) = kernel.take_pending(parent.task) {
        if let GuestAction::Fork { child: task, .. } = action {
            child = Some(task);
        }
    }
    P::new(child.expect("fork left the child"))
}

fn unshare(kernel: &Kernel, p: &P, flags: CloneFlags) -> SysResult<i32> {
    block_on(kernel.unshare(&p.caller(), flags.0 as u32))
}

/// The wall clock is the machine's: root sets it, nobody else may, and the host's stays.
#[test]
fn setting_the_wall_clock_needs_cap_sys_time_and_leaves_the_host_alone() {
    let (kernel, user) = machine_as(1);
    let later = Duration::from_secs(2_000_000_000);
    assert_eq!(
        errno(settime(&kernel, &user, ClockId::CLOCK_REALTIME, ts(later))),
        Errno::EPERM
    );

    let (kernel, root) = machine_as(0);
    // `timespec64_valid_settod` comes first, then the clock's own floor.
    let bad_nsec = KernelTimespec {
        tv_sec: 2_000_000_000,
        tv_nsec: 1_000_000_000,
    };
    let set = |to| errno(settime(&kernel, &root, ClockId::CLOCK_REALTIME, to));
    assert_eq!(set(bad_nsec), Errno::EINVAL);
    assert_eq!(set(ts(Duration::from_secs(1 << 40))), Errno::EINVAL);
    assert_eq!(
        set(ts(Duration::from_secs(1))),
        Errno::EINVAL,
        "before boot"
    );
    for clock in [
        ClockId::CLOCK_MONOTONIC,
        ClockId::CLOCK_TAI,
        ClockId::CLOCK_BOOTTIME,
    ] {
        let set = settime(&kernel, &root, clock, ts(later));
        assert_eq!(errno(set), Errno::EINVAL, "{clock:?}");
    }

    assert_eq!(
        settime(&kernel, &root, ClockId::CLOCK_REALTIME, ts(later)),
        Ok(0)
    );
    for clock in [
        ClockId::CLOCK_REALTIME,
        ClockId::CLOCK_REALTIME_COARSE,
        ClockId::CLOCK_REALTIME_ALARM,
        ClockId::CLOCK_TAI,
    ] {
        assert_eq!(now(&kernel, &root, clock), Ok(later), "{clock:?}");
    }
    assert_eq!(now(&kernel, &root, ClockId::CLOCK_MONOTONIC), Ok(HOST_MONO));
    assert_eq!(TestHost.realtime(), HOST_REAL);
    // Files are stamped with the machine's time.
    let fd = open(
        &kernel,
        &root,
        "/new",
        OpenFlags::O_CREAT | OpenFlags::O_WRONLY,
    )
    .unwrap();
    assert_eq!(mtime(&kernel, &root, "/new"), later.as_secs() as i64);
    assert_eq!(block_on(kernel.close(&root.caller(), fd)), Ok(0));
}

#[test]
fn clock_adjtime_adjusts_the_wall_clock_only() {
    let (kernel, user) = machine_as(1);
    // Reading needs no privilege; the clock boots unsynchronised.
    let (state, got) = adjtime(
        &kernel,
        &user,
        ClockId::CLOCK_REALTIME,
        txc(AdjtimexModes(0)),
    )
    .unwrap();
    assert_eq!(state, ClockState::TIME_ERROR.0);
    assert_eq!(
        got.status.0 & TimexStatus::STA_UNSYNC.0,
        TimexStatus::STA_UNSYNC.0
    );
    assert_eq!(
        (got.time.tv_sec, got.tick),
        (HOST_REAL.as_secs() as i64, 10_000)
    );
    let freq = txc(AdjtimexModes::ADJ_FREQUENCY);
    assert_eq!(
        errno(adjtime(&kernel, &user, ClockId::CLOCK_REALTIME, freq)),
        Errno::EPERM
    );
    let other = |clock| errno(adjtime(&kernel, &user, clock, txc(AdjtimexModes(0))));
    assert_eq!(other(ClockId::CLOCK_MONOTONIC), Errno::EOPNOTSUPP);
    assert_eq!(other(ClockId::CLOCK_TAI), Errno::EOPNOTSUPP);
    assert_eq!(other(ClockId::CLOCK_PROCESS_CPUTIME_ID), Errno::EOPNOTSUPP);
    assert_eq!(other(ClockId(99)), Errno::EINVAL);
    let caller = user.caller();
    let fault = kernel.clock_adjtime64(
        &caller,
        ClockId::CLOCK_REALTIME,
        GuestPtrMut::new(0xffff_0000),
    );
    assert_eq!(errno(block_on(fault)), Errno::EFAULT);

    let (kernel, root) = machine_as(0);
    // The padding goes back as it came (CVE-2018-11508).
    let padded = KernelTimex {
        pad3: [0x5a5a_5a5a; 11],
        freq: 100,
        ..txc(AdjtimexModes::ADJ_FREQUENCY)
    };
    let (_, got) = adjtime(&kernel, &root, ClockId::CLOCK_REALTIME, padded).unwrap();
    assert_eq!((got.freq, got.pad3), (100, [0x5a5a_5a5a; 11]));
    // ADJ_SETOFFSET steps the clock, ADJ_TAI moves CLOCK_TAI.
    let step = KernelTimex {
        time: wasixv2_abi::KernelTimexTimeval {
            tv_sec: 10,
            tv_usec: 0,
        },
        ..txc(AdjtimexModes::ADJ_SETOFFSET)
    };
    adjtime(&kernel, &root, ClockId::CLOCK_REALTIME, step).unwrap();
    let stepped = HOST_REAL + Duration::from_secs(10);
    assert_eq!(now(&kernel, &root, ClockId::CLOCK_REALTIME), Ok(stepped));
    let tai = KernelTimex {
        constant: 37,
        ..txc(AdjtimexModes::ADJ_TAI)
    };
    adjtime(&kernel, &root, ClockId::CLOCK_REALTIME, tai).unwrap();
    let tai = stepped + Duration::from_secs(37);
    assert_eq!(now(&kernel, &root, ClockId::CLOCK_TAI), Ok(tai));
}

fn create_timer(kernel: &Kernel, p: &P, clock: ClockId, none: bool) -> SysResult<i32> {
    let event = match none {
        true => {
            let event = Sigevent {
                sigev_notify: 1,
                ..Sigevent::default()
            };
            block_on(write_value(&p.memory, GuestPtrMut::new(EVENT), &event)).unwrap();
            GuestPtr::new(EVENT)
        }
        false => GuestPtr::NULL,
    };
    let id = GuestPtrMut::new(TIMER_ID);
    block_on(kernel.timer_create(&p.caller(), clock, event, id))?;
    Ok(block_on(read_value(&p.memory, GuestPtr::<i32>::new(TIMER_ID))).unwrap())
}

/// `alarmtimer.c`: the alarm clocks read their base clocks, and sleeping or arming a timer
/// on them needs `CAP_WAKE_ALARM`. A machine never suspends, so they run as those clocks.
#[test]
fn alarm_clocks_need_cap_wake_alarm_and_run_as_their_base_clocks() {
    let alarms = [ClockId::CLOCK_REALTIME_ALARM, ClockId::CLOCK_BOOTTIME_ALARM];
    let (kernel, user) = machine_as(1);
    let sleep = |kernel: &Kernel, p: &P, clock| {
        let rq = GuestPtr::new(TS);
        block_on(write_value(
            &p.memory,
            GuestPtrMut::new(TS),
            &ts(Duration::ZERO),
        ))
        .unwrap();
        let caller = p.caller();
        let call =
            kernel.clock_nanosleep_time64(&caller, clock, TimerFlags(0), rq, GuestPtrMut::NULL);
        block_on(call)
    };
    for clock in alarms {
        let res = block_on(kernel.clock_getres_time64(&user.caller(), clock, GuestPtrMut::new(TS)));
        assert_eq!(res, Ok(0));
        let got = block_on(read_value(
            &user.memory,
            GuestPtr::<KernelTimespec>::new(TS),
        ));
        assert_eq!(got.unwrap(), ts(Duration::from_nanos(1)));
        assert_eq!(
            errno(create_timer(&kernel, &user, clock, false)),
            Errno::EPERM
        );
        let fd = block_on(kernel.timerfd_create(&user.caller(), clock, TimerfdFlags(0)));
        assert_eq!(errno(fd), Errno::EPERM);
        assert_eq!(errno(sleep(&kernel, &user, clock)), Errno::EPERM);
    }
    assert_eq!(
        now(&kernel, &user, ClockId::CLOCK_BOOTTIME_ALARM),
        now(&kernel, &user, ClockId::CLOCK_BOOTTIME)
    );
    assert_eq!(
        now(&kernel, &user, ClockId::CLOCK_REALTIME_ALARM),
        now(&kernel, &user, ClockId::CLOCK_REALTIME)
    );

    let (kernel, root) = machine_as(0);
    for clock in alarms {
        assert!(create_timer(&kernel, &root, clock, false).is_ok());
        assert!(block_on(kernel.timerfd_create(&root.caller(), clock, TimerfdFlags(0))).is_ok());
        assert_eq!(sleep(&kernel, &root, clock), Ok(0));
        let caller = root.caller();
        let call = kernel.clock_nanosleep_time64(
            &caller,
            clock,
            TimerFlags(2),
            GuestPtr::new(TS),
            GuestPtrMut::NULL,
        );
        assert_eq!(errno(block_on(call)), Errno::EINVAL, "unknown flags");
    }
}

/// `kernel/time/namespace.c`: a new time namespace is for the children; its offsets are
/// written through `/proc/<pid>/timens_offsets` until a task enters it, and apply to the
/// monotonic and boot-time clocks of its tasks, `/proc/uptime` and absolute timer settings.
#[test]
fn a_time_namespace_offsets_its_tasks_monotonic_and_boottime_clocks() {
    let (kernel, user) = machine_as(1);
    // The initial namespace's offsets are frozen; changing them needs CAP_SYS_TIME first.
    let offsets = "/proc/self/timens_offsets";
    assert_eq!(
        errno(write_text(&kernel, &user, offsets, "monotonic 1 0")),
        Errno::EPERM
    );
    assert_eq!(
        errno(unshare(&kernel, &user, CloneFlags::CLONE_NEWTIME)),
        Errno::EPERM
    );

    let (kernel, root) = machine_as(0);
    let zeros = "monotonic           0         0\nboottime            0         0\n";
    assert_eq!(read_text(&kernel, &root, offsets), zeros);
    assert_eq!(
        errno(write_text(&kernel, &root, offsets, "monotonic 1 0")),
        Errno::EACCES
    );
    assert_eq!(unshare(&kernel, &root, CloneFlags::CLONE_NEWTIME), Ok(0));
    let link = |name: &str| {
        let at = path(&root, &format!("/proc/self/ns/{name}"));
        let n = block_on(kernel.readlinkat(&root.caller(), CWD, at, GuestPtrMut::new(BUF), 64));
        let mut out = vec![0; n.unwrap() as usize];
        block_on(root.memory.read(BUF, &mut out)).unwrap();
        String::from_utf8(out).unwrap()
    };
    assert_ne!(link("time"), link("time_for_children"));
    let bad = |text| errno(write_text(&kernel, &root, offsets, text));
    assert_eq!(bad("monotonic 1 1000000000"), Errno::EINVAL);
    assert_eq!(bad("realtime 1 0"), Errno::EINVAL);
    assert_eq!(bad("monotonic 1"), Errno::EINVAL);
    assert_eq!(
        bad("monotonic -10 0"),
        Errno::ERANGE,
        "before the clock's zero"
    );
    let text = "monotonic 100 0\n7 200 500";
    assert_eq!(
        write_text(&kernel, &root, offsets, text),
        Ok(text.len() as u32)
    );
    let shown = "monotonic         100         0\nboottime          200       500\n";
    assert_eq!(read_text(&kernel, &root, offsets), shown);
    // The caller itself stays where it was.
    assert_eq!(now(&kernel, &root, ClockId::CLOCK_MONOTONIC), Ok(HOST_MONO));

    let child = fork(&kernel, &root);
    let mono = HOST_MONO + Duration::from_secs(100);
    let boot = HOST_MONO + Duration::new(200, 500);
    for (clock, expected) in [
        (ClockId::CLOCK_MONOTONIC, mono),
        (ClockId::CLOCK_MONOTONIC_RAW, mono),
        (ClockId::CLOCK_MONOTONIC_COARSE, mono),
        (ClockId::CLOCK_BOOTTIME, boot),
        (ClockId::CLOCK_BOOTTIME_ALARM, boot),
        (ClockId::CLOCK_REALTIME, HOST_REAL),
    ] {
        assert_eq!(now(&kernel, &child, clock), Ok(expected), "{clock:?}");
    }
    assert_eq!(read_text(&kernel, &child, "/proc/uptime"), "201.50 0.00\n");
    assert_eq!(read_text(&kernel, &root, "/proc/uptime"), "1.50 0.00\n");
    // Entered, the offsets are fixed.
    assert_eq!(
        errno(write_text(&kernel, &root, offsets, "monotonic 1 0")),
        Errno::EACCES
    );

    // An absolute setting is on the namespace's clock: 10 s from the child's now is 10 s
    // from the host's.
    let id = create_timer(&kernel, &child, ClockId::CLOCK_MONOTONIC, true).unwrap();
    let spec = KernelItimerspec {
        it_interval: ts(Duration::ZERO),
        it_value: ts(mono + Duration::from_secs(10)),
    };
    block_on(write_value(&child.memory, GuestPtrMut::new(SPEC), &spec)).unwrap();
    let abs = TimerFlags::TIMER_ABSTIME;
    let caller = child.caller();
    let set = kernel.timer_settime64(&caller, id, abs, GuestPtr::new(SPEC), GuestPtrMut::NULL);
    assert_eq!(block_on(set), Ok(0));
    let get = kernel.timer_gettime64(&caller, id, GuestPtrMut::new(SPEC));
    assert_eq!(block_on(get), Ok(0));
    let got: KernelItimerspec = block_on(read_value(&child.memory, GuestPtr::new(SPEC))).unwrap();
    assert_eq!(got.it_value, ts(Duration::from_secs(10)));
}

/// `timens_install`: entering a time namespace with `setns` freezes its offsets.
#[test]
fn setns_into_a_time_namespace_freezes_its_offsets() {
    let (kernel, root) = machine_as(0);
    assert_eq!(unshare(&kernel, &root, CloneFlags::CLONE_NEWTIME), Ok(0));
    let offsets = "/proc/self/timens_offsets";
    assert_eq!(write_text(&kernel, &root, offsets, "boottime 5 0"), Ok(12));
    let fd = open(
        &kernel,
        &root,
        "/proc/self/ns/time_for_children",
        OpenFlags::O_RDONLY,
    );
    let fd = fd.unwrap();
    let flags = CloneFlags::CLONE_NEWTIME.0 as i32;
    assert_eq!(block_on(kernel.setns(&root.caller(), fd, flags)), Ok(0));
    let boot = HOST_MONO + Duration::from_secs(5);
    assert_eq!(now(&kernel, &root, ClockId::CLOCK_BOOTTIME), Ok(boot));
    assert_eq!(
        errno(write_text(&kernel, &root, offsets, "boottime 6 0")),
        Errno::EACCES
    );
}
