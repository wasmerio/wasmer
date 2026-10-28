//! Credentials and the machine's identity: who processes run as, what root may do that others
//! may not, and what `uname` reports.

use wasixv2_abi::{ConstUserCapDataArray, Rlimit64, UserCapDataArray, UserCapHeaderPtr};
use wasixv2_syscalls::write_value;

use super::{
    ipc::{errno, get, put},
    *,
};

const ROOT_FILE: u32 = 0x100;
const UTS: u32 = 0x1000;
const CAP_HEADER: u32 = 0x2000;
const CAP_DATA: u32 = 0x2010;
const CAPABILITY_V3: u32 = 0x2008_0522;
const UNCHANGED: u32 = u32::MAX;

/// A kernel configured by `configure`, with one process and 64 KiB of zeroed guest memory.
pub(super) fn machine_with(
    configure: impl FnOnce(Kernel) -> Kernel,
) -> (Kernel, TaskId, FlatMemory) {
    let host: Arc<dyn Host> = Arc::new(TestHost);
    let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1);
    let kernel = configure(Kernel::new(host, Arc::new(root)));
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
    (kernel, task, FlatMemory(RefCell::new(vec![0; 65536])))
}

pub(super) fn machine_as(uid: u32, gid: u32) -> (Kernel, TaskId, FlatMemory) {
    machine_with(|kernel| kernel.with_credentials(Credentials::new(uid, gid)))
}

/// `capget` of the caller: (effective, permitted, inheritable), 64 bits each.
fn capget(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory) -> [u64; 3] {
    put(memory, CAP_HEADER, &CAPABILITY_V3.to_le_bytes());
    put(memory, CAP_HEADER + 4, &0u32.to_le_bytes());
    let header = UserCapHeaderPtr::new(CAP_HEADER);
    let data = UserCapDataArray::new(CAP_DATA);
    assert_eq!(block_on(kernel.capget(caller, header, data)), Ok(0));
    let set = |i: u32| u64::from(word(memory, CAP_DATA + i * 4));
    let next = |i: u32| u64::from(word(memory, CAP_DATA + 12 + i * 4)) << 32;
    [0, 1, 2].map(|i| set(i) | next(i))
}

fn uname_field(memory: &FlatMemory, field: u32) -> String {
    cstr_at(memory, UTS + field * 65)
}

#[test]
fn processes_run_as_the_configured_user() {
    let (kernel, task, memory) = machine_as(1000, 100);
    let caller = Caller::new(task, &memory);

    assert_eq!(block_on(kernel.getuid(&caller)), Ok(1000));
    assert_eq!(block_on(kernel.geteuid(&caller)), Ok(1000));
    assert_eq!(block_on(kernel.getgid(&caller)), Ok(100));
    assert_eq!(
        block_on(kernel.getgroups(&caller, 0, GidArray::new(0))),
        Ok(1)
    );
    assert_eq!(
        block_on(kernel.getgroups(&caller, 4, GidArray::new(0x400))),
        Ok(1)
    );
    assert_eq!(word(&memory, 0x400), 100);
    assert_eq!(capget(&kernel, &caller, &memory), [0; 3]);
}

#[test]
fn the_default_user_is_uid_and_gid_one() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 64]));
    let caller = Caller::new(task, &memory);

    assert_eq!(block_on(kernel.getuid(&caller)), Ok(1));
    assert_eq!(block_on(kernel.getegid(&caller)), Ok(1));
}

#[test]
fn root_holds_every_capability_until_it_gives_up_uid_zero() {
    let (kernel, task, memory) = machine_as(0, 0);
    let caller = Caller::new(task, &memory);
    let full = (1u64 << 41) - 1;

    assert_eq!(capget(&kernel, &caller, &memory), [full, full, 0]);
    let read_bset =
        |cap| block_on(kernel.prctl(&caller, PrctlOption::PR_CAPBSET_READ, cap, 0, 0, 0));
    assert_eq!(read_bset(21), Ok(1));
    assert_eq!(errno(read_bset(41)), Errno::EINVAL);

    // seteuid(1000) keeps the permitted set, so seteuid(0) works.
    let seteuid = |uid| block_on(kernel.setresuid(&caller, UNCHANGED, uid, UNCHANGED));
    assert_eq!(seteuid(1000), Ok(0));
    assert_eq!(capget(&kernel, &caller, &memory), [0, full, 0]);
    assert_eq!(seteuid(0), Ok(0));
    assert_eq!(capget(&kernel, &caller, &memory), [full, full, 0]);

    // setuid(1000) does not.
    assert_eq!(block_on(kernel.setuid(&caller, 1000)), Ok(0));
    assert_eq!(capget(&kernel, &caller, &memory), [0; 3]);
    assert_eq!(errno(block_on(kernel.setuid(&caller, 0))), Errno::EPERM);
    put(&memory, 0x400, &[0; 12]);
    let ptrs = [0x400, 0x404, 0x408].map(UidPtr::new);
    assert_eq!(
        block_on(kernel.getresuid(&caller, ptrs[0], ptrs[1], ptrs[2])),
        Ok(0)
    );
    assert_eq!(get(&memory, 0x400, 12), [1000u32.to_le_bytes(); 3].concat());
}

#[test]
fn only_root_changes_groups_and_capability_sets() {
    let (kernel, task, memory) = machine_as(0, 0);
    let caller = Caller::new(task, &memory);
    put(&memory, 0x400, &[30, 0, 0, 0, 10, 0, 0, 0]);

    assert_eq!(
        block_on(kernel.setgroups(&caller, 2, ConstGidArray::new(0x400))),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.getgroups(&caller, 2, GidArray::new(0x500))),
        Ok(2)
    );
    assert_eq!(get(&memory, 0x500, 8), [10, 0, 0, 0, 30, 0, 0, 0]);

    // Keep only CAP_KILL (bit 5).
    put(&memory, CAP_DATA, &[0x20, 0, 0, 0, 0x20, 0, 0, 0]);
    put(&memory, CAP_DATA + 8, &[0; 16]);
    put(&memory, CAP_HEADER, &CAPABILITY_V3.to_le_bytes());
    let header = UserCapHeaderPtr::new(CAP_HEADER);
    let data = ConstUserCapDataArray::new(CAP_DATA);
    assert_eq!(block_on(kernel.capset(&caller, header, data)), Ok(0));
    assert_eq!(capget(&kernel, &caller, &memory), [0x20, 0x20, 0]);
    assert_eq!(
        errno(block_on(kernel.setgroups(
            &caller,
            1,
            ConstGidArray::new(0x400)
        ))),
        Errno::EPERM
    );
}

#[test]
fn capget_answers_an_unknown_version_with_its_own() {
    let (kernel, task, memory) = machine_as(1, 1);
    let caller = Caller::new(task, &memory);
    let header = UserCapHeaderPtr::new(CAP_HEADER);

    put(&memory, CAP_HEADER, &7u32.to_le_bytes());
    assert_eq!(
        block_on(kernel.capget(&caller, header, UserCapDataArray::new(0))),
        Ok(0)
    );
    assert_eq!(word(&memory, CAP_HEADER), CAPABILITY_V3);
    put(&memory, CAP_HEADER, &7u32.to_le_bytes());
    assert_eq!(
        errno(block_on(kernel.capget(
            &caller,
            header,
            UserCapDataArray::new(CAP_DATA)
        ))),
        Errno::EINVAL
    );
}

/// Root's DAC override: it reads, chmods and chowns a file another user owns with mode 0.
#[test]
fn root_bypasses_file_permissions_and_others_do_not() {
    for (uid, allowed) in [(0, true), (1, false)] {
        let (kernel, task, memory) = machine_as(uid, uid);
        let caller = Caller::new(task, &memory);
        put(&memory, ROOT_FILE, b"/tmp-f\0");
        let path = GuestPtr::new(ROOT_FILE);
        let cwd = Fd(-100);
        let create = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
        let fd = block_on(kernel.openat(&caller, cwd, path, create, 0o600)).unwrap();
        assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));

        let chown = block_on(kernel.fchownat(&caller, cwd, path, 1234, 1234, AtFlags(0)));
        if !allowed {
            assert_eq!(errno(chown), Errno::EPERM);
            continue;
        }
        assert_eq!(chown, Ok(0));
        assert_eq!(block_on(kernel.fchmodat(&caller, cwd, path, 0)), Ok(0));
        let read = block_on(kernel.openat(&caller, cwd, path, OpenFlags::O_RDONLY, 0));
        assert!(read.is_ok(), "root opens a mode-0 file: {read:?}");
        // `access` checks the real uid, which is root's too.
        let access = block_on(kernel.faccessat2(&caller, cwd, path, AccessMode(4), AtFlags(0)));
        assert_eq!(access, Ok(0));
    }
}

/// SIGCONT between processes of different users compares their sessions. Sent both ways at
/// once, the comparison must not hold one process's lock while it waits for the other's.
#[test]
fn sigcont_both_ways_between_users_does_not_deadlock() {
    let (kernel, a, memory) = machine_as(0, 0);
    let b = kernel
        .spawn(ProcessConfig {
            args: vec!["b".into()],
            env: Vec::new(),
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        })
        .unwrap();
    assert_eq!(
        block_on(kernel.setuid(&Caller::new(b, &memory), 2000)),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.setuid(&Caller::new(a, &memory), 1000)),
        Ok(0)
    );
    let kernel = Arc::new(kernel);
    let (done, finished) = std::sync::mpsc::channel();

    for (from, to) in [(a, b), (b, a)] {
        let (kernel, done) = (kernel.clone(), done.clone());
        std::thread::spawn(move || {
            let memory = FlatMemory(RefCell::new(vec![0; 64]));
            let caller = Caller::new(from, &memory);
            for _ in 0..20_000 {
                let kill = Syscalls::kill(&*kernel, &caller, to.0 as Pid, Signal::SIGCONT);
                assert_eq!(errno(block_on(kill)), Errno::EPERM);
            }
            done.send(()).unwrap();
        });
    }

    for _ in 0..2 {
        let timeout = std::time::Duration::from_secs(60);
        finished
            .recv_timeout(timeout)
            .expect("the senders deadlocked");
    }
}

#[test]
fn signals_need_a_shared_uid_or_cap_kill() {
    let (kernel, a, memory) = machine_as(0, 0);
    let b = kernel
        .spawn(ProcessConfig {
            args: vec!["b".into()],
            env: Vec::new(),
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        })
        .unwrap();
    let (a_caller, b_caller) = (Caller::new(a, &memory), Caller::new(b, &memory));
    let b_pid = b.0 as Pid;

    // Root may signal anyone; probe with signal 0.
    assert_eq!(block_on(kernel.setuid(&b_caller, 2000)), Ok(0));
    assert_eq!(
        block_on(Syscalls::kill(&kernel, &a_caller, b_pid, Signal(0))),
        Ok(0)
    );
    assert_eq!(block_on(kernel.setuid(&a_caller, 1000)), Ok(0));
    assert_eq!(
        errno(block_on(Syscalls::kill(
            &kernel,
            &a_caller,
            b_pid,
            Signal(0)
        ))),
        Errno::EPERM
    );
    assert_eq!(
        errno(block_on(kernel.tgkill(&a_caller, b_pid, b_pid, Signal(0)))),
        Errno::EPERM
    );
}

#[test]
fn raising_a_hard_limit_needs_cap_sys_resource() {
    for (uid, expected) in [(0, Ok(0)), (1, Err(SysErr::Errno(Errno::EPERM)))] {
        let (kernel, task, memory) = machine_as(uid, uid);
        let caller = Caller::new(task, &memory);
        let limit = Rlimit64 {
            rlim_cur: 1 << 20,
            rlim_max: 1 << 20,
        };
        let written = write_value(&memory, Rlimit64Ptr::new(0x400), &limit);
        assert_eq!(block_on(written), Ok(()));
        let set = kernel.prlimit64(
            &caller,
            0,
            RlimitResource::RLIMIT_NOFILE,
            ConstRlimit64Ptr::new(0x400),
            Rlimit64Ptr::new(0),
        );
        assert_eq!(block_on(set), expected, "uid {uid}");
    }
}

#[test]
fn uname_reports_the_configured_names_and_root_may_rename_the_host() {
    let uname = Uname::default()
        .with_release("6.1.0")
        .and_then(|u| u.with_nodename("box"))
        .unwrap();
    let (kernel, task, memory) = machine_with(|kernel| {
        kernel
            .with_uname(&uname)
            .with_credentials(Credentials::new(0, 0))
    });
    let caller = Caller::new(task, &memory);

    assert_eq!(
        block_on(kernel.uname(&caller, NewUtsnamePtr::new(UTS))),
        Ok(0)
    );
    assert_eq!(uname_field(&memory, 0), "wasixv2");
    assert_eq!(uname_field(&memory, 1), "box");
    assert_eq!(uname_field(&memory, 2), "6.1.0");
    assert_eq!(uname_field(&memory, 4), "wasm32");

    put(&memory, 0x400, b"renamed");
    let name = ConstBuf::new(0x400);
    assert_eq!(block_on(kernel.sethostname(&caller, name, 7)), Ok(0));
    assert_eq!(
        errno(block_on(kernel.sethostname(&caller, name, 65))),
        Errno::EINVAL
    );
    assert_eq!(
        block_on(kernel.uname(&caller, NewUtsnamePtr::new(UTS))),
        Ok(0)
    );
    assert_eq!(uname_field(&memory, 1), "renamed");

    assert_eq!(block_on(kernel.setuid(&caller, 1)), Ok(0));
    assert_eq!(
        errno(block_on(kernel.sethostname(&caller, name, 7))),
        Errno::EPERM
    );
}

#[test]
fn uname_fields_must_fit_new_utsname() {
    assert!(Uname::default().with_release("x".repeat(64)).is_ok());
    assert!(Uname::default().with_release("x".repeat(65)).is_err());
    assert!(Uname::default().with_nodename("a\0b").is_err());
}

#[test]
fn scheduling_another_users_task_needs_cap_sys_nice() {
    use wasixv2_abi::PrioWhich;

    let (kernel, a, memory) = machine_as(0, 0);
    let b = kernel
        .spawn(ProcessConfig {
            args: vec!["b".into()],
            env: Vec::new(),
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        })
        .unwrap();
    let (a_caller, b_caller) = (Caller::new(a, &memory), Caller::new(b, &memory));
    let b_pid = b.0 as i32;
    assert_eq!(block_on(kernel.setuid(&b_caller, 2000)), Ok(0));

    // Root renices anyone, and PRIO_USER finds the tasks by real uid.
    let user = PrioWhich::PRIO_USER;
    assert_eq!(kernel.sys_setpriority(&a_caller, user, 2000, 5), Ok(0));
    assert_eq!(kernel.sys_getpriority(&a_caller, user, 2000), Ok(15));
    assert_eq!(
        errno(kernel.sys_getpriority(&a_caller, user, 3000)),
        Errno::ESRCH
    );

    assert_eq!(block_on(kernel.setuid(&a_caller, 1000)), Ok(0));
    let process = PrioWhich::PRIO_PROCESS;
    assert_eq!(
        errno(kernel.sys_setpriority(&a_caller, process, b_pid, 10)),
        Errno::EPERM
    );
    block_on(memory.write(0x100, &0i32.to_le_bytes())).unwrap();
    let param = GuestPtr::new(0x100);
    let policy = SchedPolicy::SCHED_BATCH;
    let set = kernel.sched_setscheduler(&a_caller, b_pid, policy, param);
    assert_eq!(errno(block_on(set)), Errno::EPERM);
    // Its own tasks it still changes.
    let set = kernel.sched_setscheduler(&a_caller, 0, policy, param);
    assert_eq!(block_on(set), Ok(0));
}
