//! Pid namespaces: pids per namespace and their reuse, the namespace's init, signals and waits
//! across namespaces, and `/proc` bound to a namespace.

use wasixv2_abi::{
    CloneArgs, CloneFlags, Idtype, IoctlRequest, KernelTimespec, MountFlags as MsFlags, RusagePtr,
    Siginfo, SiginfoPtr, Signal, SigprocmaskHow, Sigset, WaitOptions,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, get, put},
    proc::ForkableMemory,
    *,
};

const CWD: Fd = Fd(-100);
const SIGCHLD: u32 = Signal::SIGCHLD.0 as u32;
const NEWPID: u32 = CloneFlags::CLONE_NEWPID.0 as u32;

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

/// A kernel with its init and `/proc`, and one process, run as root.
fn machine() -> (Kernel, P) {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        kernel.with_credentials(Credentials::new(0, 0)).with_init()
    });
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

/// `clone` with `flags` and SIGCHLD: the child, and the pid clone returned.
fn fork(kernel: &Kernel, parent: &P, flags: u32) -> SysResult<(P, Pid)> {
    let null = GuestPtrMut::NULL;
    let pid = block_on(kernel.clone(&parent.caller(), flags | SIGCHLD, 0, null, 0, null))?;
    let mut child = None;
    while let Some(action) = kernel.take_pending(parent.task) {
        if let GuestAction::Fork { child: task, .. } = action {
            child = Some(task);
        }
    }
    Ok((P::new(child.expect("fork left the child")), pid))
}

fn getpid(kernel: &Kernel, p: &P) -> Pid {
    block_on(kernel.getpid(&p.caller())).unwrap()
}

fn getppid(kernel: &Kernel, p: &P) -> Pid {
    block_on(kernel.getppid(&p.caller())).unwrap()
}

fn kill(kernel: &Kernel, p: &P, pid: Pid, sig: Signal) -> SysResult<i32> {
    block_on(kernel.sys_kill(&p.caller(), pid, sig))
}

/// The process exits with `status`, as its last task does, which the execution layer then
/// releases.
fn exit(kernel: &Kernel, p: &P, status: i32) {
    block_on(kernel.exit_group(&p.caller(), status)).unwrap();
    kernel.release(p.task);
}

/// What the process does on its next return to the guest: whether it dies, as a fatal signal
/// makes it, leaving its memory, and is released.
fn dies(kernel: &Kernel, p: &P) -> bool {
    match kernel.take_pending(p.task) {
        Some(GuestAction::Exit(_)) => {
            block_on(kernel.exit_mm(&p.caller()));
            kernel.release(p.task);
            true
        }
        _ => false,
    }
}

/// `waitid(P_PID, pid, WEXITED | WNOHANG)`: `si_pid`, `si_code` and `si_status` of the child
/// it reaped, if one was a zombie.
fn reap(kernel: &Kernel, p: &P, pid: Pid) -> SysResult<Option<(i32, i32, i32)>> {
    let options = WaitOptions(WaitOptions::WEXITED.0 | WaitOptions::WNOHANG.0);
    let info = SiginfoPtr::new(0x300);
    let caller = p.caller();
    block_on(kernel.waitid(&caller, Idtype::P_PID, pid, info, options, RusagePtr::NULL))?;
    let info: Siginfo = block_on(read_value(&p.memory, GuestPtr::new(0x300))).unwrap();
    let field = |at: usize| i32::from_le_bytes(info.sifields[at..at + 4].try_into().unwrap());
    Ok((info.si_signo != 0).then(|| (field(0), info.si_code, field(8))))
}

fn sigset(sig: Signal) -> Sigset {
    let mask = 1u64 << (sig.0 - 1);
    Sigset {
        sig: [mask as u32, (mask >> 32) as u32],
    }
}

fn block(kernel: &Kernel, p: &P, sig: Signal) {
    block_on(write_value(
        &p.memory,
        GuestPtrMut::new(0x160),
        &sigset(sig),
    ))
    .unwrap();
    let how = SigprocmaskHow::SIG_BLOCK;
    let caller = p.caller();
    let set = kernel.rt_sigprocmask(&caller, how, GuestPtr::new(0x160), GuestPtrMut::NULL, 8);
    assert_eq!(block_on(set), Ok(0));
}

/// `sigtimedwait` for `sig` without waiting: the `si_pid` of the signal it took.
fn take_si_pid(kernel: &Kernel, p: &P, sig: Signal) -> Option<i32> {
    let now = KernelTimespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    block_on(write_value(
        &p.memory,
        GuestPtrMut::new(0x170),
        &sigset(sig),
    ))
    .unwrap();
    block_on(write_value(&p.memory, GuestPtrMut::new(0x178), &now)).unwrap();
    let caller = p.caller();
    let wait = kernel.rt_sigtimedwait_time64(
        &caller,
        GuestPtr::new(0x170),
        GuestPtrMut::new(0x200),
        GuestPtr::new(0x178),
        8,
    );
    block_on(wait).ok()?;
    let info: Siginfo = block_on(read_value(&p.memory, GuestPtr::new(0x200))).unwrap();
    Some(i32::from_le_bytes(info.sifields[0..4].try_into().unwrap()))
}

/// Puts `text` NUL-terminated at `addr`.
fn cstr(p: &P, addr: u32, text: &str) -> Cstr {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    put(&p.memory.0, addr, &bytes);
    GuestPtr::new(addr)
}

/// The text of the file `at`.
fn cat(kernel: &Kernel, p: &P, at: &str) -> SysResult<String> {
    let name = cstr(p, 0x1000, at);
    let caller = p.caller();
    let fd = block_on(kernel.openat(&caller, CWD, name, OpenFlags::O_RDONLY, 0))?;
    let mut out = Vec::new();
    loop {
        let n = block_on(kernel.read(&caller, fd, UserSliceMut::new(0x4000, 4096))).unwrap();
        if n == 0 {
            break;
        }
        out.extend(get(&p.memory.0, 0x4000, n as usize));
    }
    block_on(kernel.close(&caller, fd)).unwrap();
    Ok(String::from_utf8(out).unwrap())
}

/// Writes `text` to the file `at`.
fn echo(kernel: &Kernel, p: &P, at: &str, text: &str) -> SysResult<i32> {
    let name = cstr(p, 0x1000, at);
    let caller = p.caller();
    let fd = block_on(kernel.openat(&caller, CWD, name, OpenFlags::O_WRONLY, 0))?;
    put(&p.memory.0, 0x4000, text.as_bytes());
    let written = block_on(kernel.write(&caller, fd, UserSlice::new(0x4000, text.len() as u32)));
    block_on(kernel.close(&caller, fd)).unwrap();
    written.map(|n| n as i32)
}

/// The names in the directory `at`, but `.` and `..`.
fn ls(kernel: &Kernel, p: &P, at: &str) -> Vec<String> {
    let name = cstr(p, 0x1000, at);
    let caller = p.caller();
    let flags = OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY;
    let dir = block_on(kernel.openat(&caller, CWD, name, flags, 0)).unwrap();
    let mut names = Vec::new();
    loop {
        let n = block_on(kernel.getdents64(&caller, dir, GuestPtrMut::new(0x4000), 4096)).unwrap();
        if n == 0 {
            break;
        }
        let records = get(&p.memory.0, 0x4000, n as usize);
        let mut at = 0;
        while at < records.len() {
            let reclen = u16::from_le_bytes([records[at + 16], records[at + 17]]) as usize;
            let name = &records[at + 19..at + reclen];
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            names.push(String::from_utf8_lossy(&name[..end]).into_owned());
            at += reclen;
        }
    }
    block_on(kernel.close(&caller, dir)).unwrap();
    names.retain(|name| name != "." && name != "..");
    names
}

/// The line of `/proc/<pid>/status` that starts with `key`, without it.
fn status(kernel: &Kernel, p: &P, pid: &str, key: &str) -> String {
    let text = cat(kernel, p, &format!("/proc/{pid}/status")).unwrap();
    let line = text.lines().find(|line| line.starts_with(key)).unwrap();
    line[key.len()..].trim().to_owned()
}

#[test]
fn the_first_child_in_a_new_pid_namespace_is_its_init() {
    let (kernel, parent) = machine();

    let (child, pid) = fork(&kernel, &parent, NEWPID).unwrap();

    // pidns01: the parent sees its child's pid in its own namespace.
    assert!(pid > 1);
    assert_eq!(getpid(&kernel, &child), 1);
    assert_eq!(block_on(kernel.gettid(&child.caller())), Ok(1));
    assert_eq!(getppid(&kernel, &child), 0, "the parent is outside");
    // pidns02: its group and session are its parent's, outside too.
    let caller = child.caller();
    assert_eq!(block_on(kernel.getsid(&caller, 0)), Ok(0));
    assert_eq!(block_on(kernel.getpgid(&caller, 0)), Ok(0));
    assert_eq!(block_on(kernel.setsid(&caller)), Ok(1));
    assert_eq!(block_on(kernel.getsid(&caller, 0)), Ok(1));
    assert_eq!(block_on(kernel.getpgid(&caller, 0)), Ok(1));
    // The parent sees the new session by the child's pid.
    assert_eq!(block_on(kernel.getsid(&parent.caller(), pid)), Ok(pid));
}

#[test]
fn unshare_newpid_moves_only_the_children() {
    let (kernel, parent) = machine();
    let before = getpid(&kernel, &parent);

    assert_eq!(block_on(kernel.unshare(&parent.caller(), NEWPID)), Ok(0));

    assert_eq!(getpid(&kernel, &parent), before);
    // The children's namespace is not the caller's own any more: no second one below it.
    assert_eq!(
        errno(block_on(kernel.unshare(&parent.caller(), NEWPID))),
        Errno::EINVAL
    );
    let pid_link = |task: &P, link: &str| {
        let name = cstr(task, 0x1000, &format!("/proc/self/ns/{link}"));
        let out = GuestPtrMut::new(0x2000);
        let n = block_on(kernel.readlinkat(&task.caller(), CWD, name, out, 64)).unwrap();
        String::from_utf8(get(&task.memory.0, 0x2000, n as usize)).unwrap()
    };
    assert_ne!(
        pid_link(&parent, "pid"),
        pid_link(&parent, "pid_for_children")
    );
    let (child, pid) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(getpid(&kernel, &child), 1);
    assert_eq!(
        pid_link(&child, "pid"),
        pid_link(&parent, "pid_for_children")
    );
    let (grandchild, _) = fork(&kernel, &child, 0).unwrap();
    assert_eq!(getpid(&kernel, &grandchild), 2);
    assert_eq!(getppid(&kernel, &grandchild), 1);
    // The parent's next child is the init's sibling, not a second init.
    let (sibling, _) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(getpid(&kernel, &sibling), 3);
    assert!(pid > 1);
}

#[test]
fn pid_namespaces_nest_32_deep() {
    let (kernel, first) = machine();
    let mut inits = vec![first];
    for level in 1..=32 {
        let parent = inits.last().unwrap();
        let (child, _) = fork(&kernel, parent, NEWPID).unwrap_or_else(|_| panic!("level {level}"));
        inits.push(child);
    }

    let deepest = inits.last().unwrap();
    assert_eq!(
        fork(&kernel, deepest, NEWPID).err(),
        Some(SysErr::Errno(Errno::ENOSPC))
    );
    let (child, _) = fork(&kernel, deepest, 0).unwrap();
    assert_eq!(getpid(&kernel, &child), 2);
    let nspid = status(&kernel, &child, "self", "NSpid:");
    assert_eq!(nspid.split('\t').count(), 33, "one pid per level: {nspid}");
    assert!(nspid.ends_with("\t3\t2"), "{nspid}");
}

#[test]
fn a_new_pid_namespace_needs_cap_sys_admin_and_no_thread() {
    let (kernel, task, _) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(1, 1)));
    let user = P::new(task);
    assert_eq!(
        fork(&kernel, &user, NEWPID).err(),
        Some(SysErr::Errno(Errno::EPERM))
    );

    let (kernel, root) = machine();
    let thread = CloneFlags::CLONE_THREAD.0 as u32
        | CloneFlags::CLONE_SIGHAND.0 as u32
        | CloneFlags::CLONE_VM.0 as u32;
    let null = GuestPtrMut::NULL;
    let caller = root.caller();
    let clone = kernel.clone(&caller, thread | NEWPID, 0, null, 0, null);
    assert_eq!(errno(block_on(clone)), Errno::EINVAL);
}

#[test]
fn orphans_go_to_the_namespace_init_and_its_children_to_nobody_inside() {
    let (kernel, parent) = machine();
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();
    let (middle, middle_pid) = fork(&kernel, &init, 0).unwrap();
    let (orphan, _) = fork(&kernel, &middle, 0).unwrap();
    assert_eq!(getppid(&kernel, &orphan), middle_pid);

    exit(&kernel, &middle, 0);

    assert_eq!(
        getppid(&kernel, &orphan),
        1,
        "reparented to the namespace's init"
    );
    let reaped = reap(&kernel, &init, middle_pid).unwrap();
    assert_eq!(
        reaped,
        Some((middle_pid, 1, 0)),
        "CLD_EXITED, as the init sees it"
    );
    // The orphan's end reaches its new parent, by its pid inside.
    exit(&kernel, &orphan, 3);
    assert_eq!(reap(&kernel, &init, -1).ok(), None, "no -1 for P_PID");
    let options = WaitOptions(WaitOptions::WEXITED.0 | WaitOptions::WNOHANG.0);
    let caller = init.caller();
    let wait = kernel.waitid(
        &caller,
        Idtype::P_ALL,
        0,
        SiginfoPtr::new(0x300),
        options,
        RusagePtr::NULL,
    );
    assert_eq!(block_on(wait), Ok(0));
    let info: Siginfo = block_on(read_value(&init.memory, GuestPtr::new(0x300))).unwrap();
    assert_eq!(
        i32::from_le_bytes(info.sifields[0..4].try_into().unwrap()),
        3,
        "si_pid inside"
    );
}

#[test]
fn signals_from_inside_reach_the_init_only_through_handlers() {
    let (kernel, parent) = machine();
    let (init, pid) = fork(&kernel, &parent, NEWPID).unwrap();

    // pidns04: SIGKILL from inside is dropped.
    assert_eq!(kill(&kernel, &init, 1, Signal::SIGKILL), Ok(0));
    assert!(!dies(&kernel, &init));
    // pidns16: a blocked signal from inside names the sender by its pid there, from the
    // parent, outside, by 0 (pidns12).
    block(&kernel, &init, Signal::SIGUSR1);
    assert_eq!(kill(&kernel, &init, 1, Signal::SIGUSR1), Ok(0));
    assert_eq!(take_si_pid(&kernel, &init, Signal::SIGUSR1), Some(1));
    assert_eq!(kill(&kernel, &parent, pid, Signal::SIGUSR1), Ok(0));
    assert_eq!(take_si_pid(&kernel, &init, Signal::SIGUSR1), Some(0));
    // pidns05: SIGKILL from outside is forced on it.
    assert_eq!(kill(&kernel, &parent, pid, Signal::SIGKILL), Ok(0));
    assert!(dies(&kernel, &init));
    assert_eq!(
        reap(&kernel, &parent, pid).unwrap(),
        Some((pid, 2, Signal::SIGKILL.0)),
        "CLD_KILLED"
    );
}

#[test]
fn a_process_inside_cannot_name_processes_outside() {
    let (kernel, parent) = machine();
    let outside = getpid(&kernel, &parent);
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();

    // pidns06.
    assert_eq!(
        errno(kill(&kernel, &init, outside, Signal::SIGKILL)),
        Errno::ESRCH
    );
    let caller = init.caller();
    assert_eq!(
        errno(block_on(kernel.getpgid(&caller, outside))),
        Errno::ESRCH
    );
    assert_eq!(
        errno(block_on(kernel.pidfd_open(&caller, outside, 0))),
        Errno::ESRCH
    );
    // pidns10: nothing else is inside.
    assert_eq!(
        errno(kill(&kernel, &init, -1, Signal::SIGKILL)),
        Errno::ESRCH
    );
}

#[test]
fn kill_minus_one_reaches_every_process_of_the_namespace_but_its_init() {
    let (kernel, parent) = machine();
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();
    let children: Vec<(P, Pid)> = (0..3).map(|_| fork(&kernel, &init, 0).unwrap()).collect();

    // pidns17.
    assert_eq!(kill(&kernel, &init, -1, Signal::SIGUSR1), Ok(0));

    for (child, pid) in &children {
        assert!(dies(&kernel, child));
        let killed = (*pid, 2, Signal::SIGUSR1.0);
        assert_eq!(reap(&kernel, &init, *pid).unwrap(), Some(killed));
    }
    assert!(!dies(&kernel, &init));
    assert!(!dies(&kernel, &parent));
}

#[test]
fn the_end_of_the_init_kills_its_namespace_and_waits_for_it() {
    let (kernel, parent) = machine();
    let (init, pid) = fork(&kernel, &parent, NEWPID).unwrap();
    let (child, _) = fork(&kernel, &init, 0).unwrap();
    let (nested, _) = fork(&kernel, &child, NEWPID).unwrap();
    let (deeper, _) = fork(&kernel, &nested, 0).unwrap();

    exit(&kernel, &init, 0);

    // Every other process of the namespace, nested ones included, gets SIGKILL, and the init
    // is no zombie while one is left.
    assert_eq!(reap(&kernel, &parent, pid), Ok(None));
    for task in [&deeper, &nested] {
        assert!(dies(&kernel, task));
    }
    assert_eq!(reap(&kernel, &parent, pid), Ok(None));
    assert!(dies(&kernel, &child));
    assert_eq!(reap(&kernel, &parent, pid), Ok(Some((pid, 1, 0))));
    // The namespace is empty and takes no process any more (`PIDNS_ADDING`).
    let ns = kernel.procs.find(init.task.0);
    assert!(ns.is_none(), "reaped");
}

#[test]
fn a_process_that_enters_a_dead_namespace_cannot_fork_into_it() {
    let (kernel, parent) = machine();
    assert_eq!(block_on(kernel.unshare(&parent.caller(), NEWPID)), Ok(0));
    let (init, pid) = fork(&kernel, &parent, 0).unwrap();
    exit(&kernel, &init, 0);
    assert_eq!(reap(&kernel, &parent, pid), Ok(Some((pid, 1, 0))));

    assert_eq!(
        fork(&kernel, &parent, 0).err(),
        Some(SysErr::Errno(Errno::ENOMEM))
    );
}

#[test]
fn an_init_cannot_make_siblings() {
    let (kernel, parent) = machine();
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();
    let flags = CloneFlags::CLONE_PARENT.0 as u32;
    assert_eq!(
        fork(&kernel, &init, flags).err(),
        Some(SysErr::Errno(Errno::EINVAL))
    );
}

#[test]
fn setns_into_a_child_pid_namespace_moves_the_next_children() {
    let (kernel, parent) = machine();
    let (init, init_pid) = fork(&kernel, &parent, NEWPID).unwrap();
    let path = format!("/proc/{init_pid}/ns/pid");
    let name = cstr(&parent, 0x1000, &path);
    let caller = parent.caller();
    let fd = block_on(kernel.openat(&caller, CWD, name, OpenFlags::O_RDONLY, 0)).unwrap();

    assert_eq!(block_on(kernel.setns(&caller, fd, NEWPID as i32)), Ok(0));

    let (injected, pid) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(getpid(&kernel, &injected), 2);
    assert_eq!(getppid(&kernel, &injected), 0, "its parent is outside");
    assert_ne!(pid, 2, "the parent sees its own pid for it");
    // The init cannot enter its parent's namespace, above its own (`pidns_install`).
    let up = cstr(&parent, 0x1000, "/proc/self/ns/pid");
    let fd = block_on(kernel.openat(&caller, CWD, up, OpenFlags::O_RDONLY, 0)).unwrap();
    let path = format!("/proc/{}/fd/{}", getpid(&kernel, &parent), fd.0);
    let name = cstr(&init, 0x1000, &path);
    let init_caller = init.caller();
    let up = block_on(kernel.openat(&init_caller, CWD, name, OpenFlags::O_RDONLY, 0)).unwrap();
    assert_eq!(
        errno(block_on(kernel.setns(&init_caller, up, NEWPID as i32))),
        Errno::EINVAL
    );
}

#[test]
fn the_parent_hears_of_a_child_in_another_namespace_by_its_own_pid() {
    let (kernel, parent) = machine();
    block(&kernel, &parent, Signal::SIGCHLD);
    let (init, pid) = fork(&kernel, &parent, NEWPID).unwrap();

    exit(&kernel, &init, 5);

    assert_eq!(take_si_pid(&kernel, &parent, Signal::SIGCHLD), Some(pid));
    assert_eq!(reap(&kernel, &parent, pid), Ok(Some((pid, 1, 5))));
}

#[test]
fn pids_are_reused_once_free_and_wrap_at_pid_max() {
    let (kernel, parent) = machine();
    assert_eq!(
        cat(&kernel, &parent, "/proc/sys/kernel/pid_max"),
        Ok("32768\n".into())
    );
    let last = getpid(&kernel, &parent);
    assert_eq!(
        cat(&kernel, &parent, "/proc/sys/kernel/ns_last_pid"),
        Ok(format!("{last}\n"))
    );

    // `ns_last_pid` sets where the next search starts.
    assert_eq!(
        echo(&kernel, &parent, "/proc/sys/kernel/ns_last_pid", "32765\n"),
        Ok(6)
    );
    let (a, a_pid) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(a_pid, 32766);
    let (b, b_pid) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(b_pid, 32767);
    // Past `pid_max`, allocation wraps to `RESERVED_PIDS`.
    let (_, wrapped) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(wrapped, 300);
    assert_eq!(getpid(&kernel, &a), 32766);
    // A reaped pid is free again, under a new id.
    exit(&kernel, &b, 0);
    assert_eq!(reap(&kernel, &parent, b_pid), Ok(Some((b_pid, 1, 0))));
    assert_eq!(
        echo(&kernel, &parent, "/proc/sys/kernel/ns_last_pid", "32766"),
        Ok(5)
    );
    let (reused, reused_pid) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(reused_pid, 32767);
    assert_ne!(reused.task, b.task);
    assert_eq!(kill(&kernel, &parent, b_pid, Signal::SIGUSR1), Ok(0));
    assert!(dies(&kernel, &reused), "the pid names the new process");
    // A smaller `pid_max` keeps new pids below it.
    assert_eq!(
        echo(&kernel, &parent, "/proc/sys/kernel/pid_max", "1000\n"),
        Ok(5)
    );
    assert_eq!(
        errno(echo(&kernel, &parent, "/proc/sys/kernel/pid_max", "300")),
        Errno::EINVAL
    );
    assert_eq!(
        echo(&kernel, &parent, "/proc/sys/kernel/ns_last_pid", "998"),
        Ok(3)
    );
    let (_, below) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(below, 999);
    let (_, again) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(again, 301, "300 is taken");
}

#[test]
fn reaping_frees_the_pid_before_the_execution_layer_releases_the_task() {
    let (kernel, parent) = machine();
    let (child, pid) = fork(&kernel, &parent, 0).unwrap();
    // The child is a zombie once it exits, while its instance still unwinds: the execution
    // layer has not released its task yet.
    block_on(kernel.exit_group(&child.caller(), 0)).unwrap();
    assert_eq!(reap(&kernel, &parent, pid), Ok(Some((pid, 1, 0))));
    // `release_task` at the reap detaches the pid, so nothing finds it and it is free again.
    let getpgid = |pid| block_on(kernel.getpgid(&parent.caller(), pid));
    assert_eq!(errno(getpgid(pid)), Errno::ESRCH);
    assert_eq!(
        errno(kill(&kernel, &parent, pid, Signal::SIGUSR1)),
        Errno::ESRCH
    );
    let last = format!("{}", pid - 1);
    assert_eq!(
        echo(&kernel, &parent, "/proc/sys/kernel/ns_last_pid", &last),
        Ok(last.len() as i32)
    );
    let (reused, reused_pid) = fork(&kernel, &parent, 0).unwrap();
    assert_eq!(reused_pid, pid);
    // The old task's release leaves the new process's pid alone.
    kernel.release(child.task);
    assert_eq!(getpgid(pid), getpgid(0));
    assert_eq!(getpid(&kernel, &reused), pid);
}

#[test]
fn a_new_namespace_counts_from_one_and_shows_its_own_last_pid() {
    let (kernel, parent) = machine();
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();
    let (child, _) = fork(&kernel, &init, 0).unwrap();

    assert_eq!(getpid(&kernel, &child), 2);
    assert_eq!(
        cat(&kernel, &child, "/proc/sys/kernel/ns_last_pid"),
        Ok("2\n".into())
    );
    assert_eq!(
        cat(&kernel, &child, "/proc/sys/kernel/pid_max"),
        Ok("4194304\n".into())
    );
    let outside = getpid(&kernel, &parent);
    assert_ne!(
        cat(&kernel, &parent, "/proc/sys/kernel/ns_last_pid"),
        Ok("2\n".into()),
        "{outside}"
    );
}

#[test]
fn clone3_set_tid_picks_the_pids_from_the_innermost_namespace_up() {
    let (kernel, parent) = machine();
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();
    let clone3 = |p: &P, tids: &[i32]| -> SysResult<i32> {
        let raw: Vec<u8> = tids.iter().flat_map(|t| t.to_le_bytes()).collect();
        put(&p.memory.0, 0x5000, &raw);
        let args = CloneArgs {
            exit_signal: SIGCHLD as u64,
            set_tid: 0x5000,
            set_tid_size: tids.len() as u64,
            ..CloneArgs::default()
        };
        block_on(write_value(&p.memory, GuestPtrMut::new(0x6000), &args)).unwrap();
        let size = std::mem::size_of::<CloneArgs>() as u32;
        let ret = block_on(kernel.clone3(&p.caller(), GuestPtr::new(0x6000), size));
        while kernel.take_pending(p.task).is_some() {}
        ret
    };

    assert_eq!(clone3(&init, &[7]), Ok(7));
    assert_eq!(errno(clone3(&init, &[7])), Errno::EEXIST);
    assert_eq!(errno(clone3(&init, &[0])), Errno::EINVAL);
    assert_eq!(
        errno(clone3(&init, &[8, 9, 10])),
        Errno::EINVAL,
        "two levels only"
    );
    let outside = clone3(&init, &[8, 5000]).unwrap();
    assert_eq!(outside, 8);
    let pids = ls(&kernel, &parent, "/proc");
    assert!(pids.iter().any(|name| name == "5000"), "{pids:?}");
}

#[test]
fn proc_mounted_in_a_namespace_lists_its_processes_by_their_pids_there() {
    let (kernel, parent) = machine();
    let flags = NEWPID | CloneFlags::CLONE_NEWNS.0 as u32;
    let (init, pid) = fork(&kernel, &parent, flags).unwrap();
    let (child, child_pid) = fork(&kernel, &init, 0).unwrap();
    let null = GuestPtrMut::NULL;
    let caller = child.caller();
    let thread_pid = block_on(kernel.thread_spawn(&caller, 0x10f00, 0, null, 0, null)).unwrap();
    while kernel.take_pending(child.task).is_some() {}

    // The mount the namespace inherited still shows the parent's view.
    assert_eq!(
        cat(&kernel, &init, "/proc/self/status").map(|s| s.contains(&format!("Pid:\t{pid}\n"))),
        Ok(true)
    );
    let source = cstr(&init, 0x1100, "proc");
    let target = cstr(&init, 0x1200, "/proc");
    let fstype = cstr(&init, 0x1300, "proc");
    let caller = init.caller();
    let mount = kernel.mount(&caller, source, target, fstype, MsFlags(0), GuestPtr::NULL);
    assert_eq!(block_on(mount), Ok(0));

    let mut pids: Vec<String> = ls(&kernel, &init, "/proc")
        .into_iter()
        .filter(|name| name.parse::<u32>().is_ok())
        .collect();
    pids.sort();
    assert_eq!(pids, ["1", "2"]);
    assert_eq!(ls(&kernel, &init, "/proc/2/task"), ["2", "3"]);
    assert_eq!(thread_pid, 3);
    assert_eq!(child_pid, 2);
    let link = |p: &P, at: &str| {
        let name = cstr(p, 0x1000, at);
        let out = GuestPtrMut::new(0x2000);
        let n = block_on(kernel.readlinkat(&p.caller(), CWD, name, out, 64)).unwrap();
        String::from_utf8(get(&p.memory.0, 0x2000, n as usize)).unwrap()
    };
    assert_eq!(link(&child, "/proc/self"), "2");
    assert_eq!(status(&kernel, &child, "self", "PPid:"), "1");
    assert_eq!(status(&kernel, &child, "2", "NSpid:"), "2");
    let stat = cat(&kernel, &init, "/proc/1/stat").unwrap();
    assert!(stat.starts_with("1 ("), "{stat}");
    assert_eq!(stat.split(' ').nth(3), Some("0"), "ppid outside: {stat}");
    // The parent's `/proc` shows the namespace's processes by their pids outside, and every
    // pid they have below.
    let outside = status(
        &kernel,
        &parent,
        &child_pid_outside(&kernel, &child),
        "NSpid:",
    );
    assert!(outside.ends_with("\t2"), "{outside}");
    assert_eq!(outside.split('\t').count(), 2);
}

/// The pid `child` has in the initial namespace.
fn child_pid_outside(kernel: &Kernel, child: &P) -> String {
    kernel
        .pids
        .nr(&kernel.init_ns.pid_for_children, child.task.0)
        .to_string()
}

/// `mq_open` of `/name`, read-write, created if `create`.
fn mq_open(kernel: &Kernel, p: &P, name: &str, create: bool) -> i32 {
    let name = cstr(p, 0x1000, name);
    let mut flags = OpenFlags::O_RDWR;
    if create {
        flags |= OpenFlags::O_CREAT;
    }
    block_on(kernel.mq_open(&p.caller(), name, flags, 0o600, GuestPtr::NULL)).unwrap()
}

/// `mq_notify` with SIGUSR1 (`SIGEV_SIGNAL`).
fn mq_notify(kernel: &Kernel, p: &P, mq: i32) {
    let mut event = [0u8; 64];
    event[4..8].copy_from_slice(&Signal::SIGUSR1.0.to_le_bytes());
    put(&p.memory.0, 0x5000, &event);
    assert_eq!(
        block_on(kernel.mq_notify(&p.caller(), mq, GuestPtr::new(0x5000))),
        Ok(0)
    );
}

fn mq_send(kernel: &Kernel, p: &P, mq: i32) {
    put(&p.memory.0, 0x5100, b"pippo");
    let caller = p.caller();
    let send = kernel.mq_timedsend_time64(&caller, mq, GuestPtr::new(0x5100), 5, 1, GuestPtr::NULL);
    assert_eq!(block_on(send), Ok(0));
}

#[test]
fn a_mq_notification_names_the_sender_as_the_owner_sees_it() {
    let (kernel, parent) = machine();
    let (init, pid) = fork(&kernel, &parent, NEWPID).unwrap();
    block(&kernel, &parent, Signal::SIGUSR1);
    block(&kernel, &init, Signal::SIGUSR1);

    // pidns31: from inside to the parent, by the sender's pid outside.
    let mq = mq_open(&kernel, &parent, "q", true);
    mq_notify(&kernel, &parent, mq);
    let mq_child = mq_open(&kernel, &init, "q", false);
    mq_send(&kernel, &init, mq_child);
    assert_eq!(take_si_pid(&kernel, &parent, Signal::SIGUSR1), Some(pid));

    // pidns30: from the parent to the init, by 0.
    let mq_child = mq_open(&kernel, &init, "r", true);
    mq_notify(&kernel, &init, mq_child);
    let mq = mq_open(&kernel, &parent, "r", false);
    mq_send(&kernel, &parent, mq);
    assert_eq!(take_si_pid(&kernel, &init, Signal::SIGUSR1), Some(0));
}

#[test]
fn the_execution_layer_gets_the_pid_each_side_sees() {
    let (kernel, parent) = machine();
    let settid = CloneFlags::CLONE_PARENT_SETTID.0 as u32 | CloneFlags::CLONE_CHILD_SETTID.0 as u32;
    let (at_parent, at_child) = (GuestPtrMut::new(0x80), GuestPtrMut::new(0x84));
    let caller = parent.caller();

    // fork: the parent's clone returns the child's pid outside, the child's tid is 1.
    let flags = NEWPID | settid | SIGCHLD;
    let pid = block_on(kernel.clone(&caller, flags, 0, at_parent, 0, at_child)).unwrap();
    match kernel.take_pending(parent.task) {
        Some(GuestAction::Fork {
            pid: parent_sees,
            tid,
            ..
        }) => assert_eq!((parent_sees, tid), (pid, 1)),
        other => panic!("{other:?}"),
    }

    // vfork: the writes land at once, and the parent's vfork returns the pid it sees.
    let vfork = CloneFlags::CLONE_VM.0 as u32 | CloneFlags::CLONE_VFORK.0 as u32;
    let flags = NEWPID | settid | vfork | SIGCHLD;
    assert_eq!(
        block_on(kernel.clone(&caller, flags, 0, at_parent, 0, at_child)),
        Ok(0)
    );
    let child = match kernel.take_pending(parent.task) {
        Some(GuestAction::SwitchTask { task, .. }) => task,
        other => panic!("{other:?}"),
    };
    let word = |at: u32| i32::from_le_bytes(get(&parent.memory.0, at, 4).try_into().unwrap());
    let outside = word(0x80);
    assert!(outside > 1);
    assert_eq!(word(0x84), 1);
    let child_caller = Caller::new(child, &parent.memory);
    block_on(kernel.exit_group(&child_caller, 0)).unwrap();
    match kernel.take_pending(child) {
        Some(GuestAction::VforkReturn { parent: back, .. }) => assert_eq!(back.pid, outside),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_init_whose_pids_could_not_all_be_made_is_no_init() {
    let (kernel, parent) = machine();
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();
    // The init's namespace has no pid left: 1 to 300 are taken.
    assert_eq!(
        echo(&kernel, &init, "/proc/sys/kernel/pid_max", "301"),
        Ok(3)
    );
    let children: Vec<(P, Pid)> = (2..=300)
        .map(|_| fork(&kernel, &init, 0).unwrap())
        .collect();
    assert_eq!(children.last().map(|(_, pid)| *pid), Some(300));
    assert_eq!(block_on(kernel.unshare(&init.caller(), NEWPID)), Ok(0));

    // The new namespace's pid 1 is made, the one around it is not.
    assert_eq!(
        fork(&kernel, &init, 0).err(),
        Some(SysErr::Errno(Errno::EAGAIN))
    );
    // Room for more.
    assert_eq!(
        echo(&kernel, &init, "/proc/sys/kernel/pid_max", "400"),
        Ok(3)
    );

    let (nested, outside) = fork(&kernel, &init, 0).unwrap();
    assert_eq!(outside, 301);
    assert_eq!(getpid(&kernel, &nested), 1, "the next try makes the init");
    let (orphan_parent, _) = fork(&kernel, &nested, 0).unwrap();
    let (orphan, _) = fork(&kernel, &orphan_parent, 0).unwrap();
    exit(&kernel, &orphan_parent, 0);
    assert_eq!(
        getppid(&kernel, &orphan),
        1,
        "and adopts the namespace's orphans"
    );
}

/// `PIDFD_GET_INFO` with a `struct pidfd_info` of `size` bytes at 0x500.
fn pidfd_info(kernel: &Kernel, p: &P, pidfd: Fd, size: u32, mask: u64) -> SysResult<i32> {
    put(&p.memory.0, 0x500, &mask.to_le_bytes());
    let request = IoctlRequest((IoctlRequest::PIDFD_GET_INFO.0 & !(0x3fff << 16)) | (size << 16));
    block_on(kernel.ioctl(&p.caller(), pidfd, request, 0x500))
}

/// The `struct pidfd_info` at 0x500.
fn pidfd_info_at(p: &P) -> wasixv2_abi::PidfdInfo {
    block_on(read_value(&p.memory, GuestPtr::new(0x500))).unwrap()
}

/// `PIDFD_GET_INFO` (`fs/pidfs.c` `pidfd_info`, LTP ioctl_pidfd02-06): the pid, thread group,
/// parent and credentials of a process not yet reaped, as the caller's namespaces see them;
/// its wait status, when asked for, once it is reaped, and nothing else then (`ESRCH`) but the
/// cgroup it exited in; its cgroup's id; `EREMOTE` for a process outside the caller's pid
/// namespace. The request takes
/// any size from the first published struct on, and fills as much as both know.
#[test]
fn pidfd_get_info_tells_a_process_and_its_exit() {
    use wasixv2_abi::PidfdInfoMask as M;
    let (kernel, parent) = machine();
    let (child, pid) = fork(&kernel, &parent, 0).unwrap();
    let pidfd = block_on(kernel.pidfd_open(&parent.caller(), pid, 0)).unwrap();
    let exit_asked = M::PIDFD_INFO_EXIT.0 | M::PIDFD_INFO_SUPPORTED_MASK.0;

    assert_eq!(pidfd_info(&kernel, &parent, pidfd, 88, exit_asked), Ok(0));
    let info = pidfd_info_at(&parent);
    let filled = M::PIDFD_INFO_PID.0
        | M::PIDFD_INFO_CREDS.0
        | M::PIDFD_INFO_CGROUPID.0
        | M::PIDFD_INFO_SUPPORTED_MASK.0;
    assert_eq!(info.mask, filled, "no exit before it is reaped");
    assert_eq!((info.pid, info.tgid), (pid as u32, pid as u32));
    assert_eq!(info.ppid, getpid(&kernel, &parent) as u32);
    assert_eq!(
        (info.ruid, info.euid, info.fsgid, info.exit_code),
        (0, 0, 0, 0)
    );
    assert_eq!(info.cgroupid, 1, "the root cgroup");
    assert_eq!(info.supported_mask, 0xff);

    // LTP's struct is the second published one, 72 bytes: nothing past it is written; one
    // past ours gets zeroes.
    put(&parent.memory.0, 0x500 + 72, &[0xee; 40]);
    assert_eq!(pidfd_info(&kernel, &parent, pidfd, 72, 0), Ok(0));
    assert_eq!(get(&parent.memory.0, 0x500 + 72, 16), [0xee; 16]);
    assert_eq!(pidfd_info(&kernel, &parent, pidfd, 104, 0), Ok(0));
    assert_eq!(get(&parent.memory.0, 0x500 + 72, 32), [0; 32]);
    assert_eq!(get(&parent.memory.0, 0x500 + 104, 8), [0xee; 8]);
    // Smaller than the first, or another request, is not this one (`pidfs_ioctl_valid`).
    assert_eq!(
        errno(pidfd_info(&kernel, &parent, pidfd, 4, 0)),
        Errno::ENOTTY
    );
    let null = block_on(kernel.ioctl(&parent.caller(), pidfd, IoctlRequest::PIDFD_GET_INFO, 0));
    assert_eq!(errno(null), Errno::EINVAL);

    // A process in a pid namespace of its own cannot ask of its parent's.
    let (init, _) = fork(&kernel, &parent, NEWPID).unwrap();
    assert_eq!(
        errno(pidfd_info(&kernel, &init, pidfd, 88, 0)),
        Errno::EREMOTE
    );

    exit(&kernel, &child, 100);
    assert_eq!(pidfd_info(&kernel, &parent, pidfd, 88, exit_asked), Ok(0));
    assert_eq!(
        pidfd_info_at(&parent).mask & M::PIDFD_INFO_EXIT.0,
        0,
        "a zombie"
    );
    assert_eq!(reap(&kernel, &parent, pid).unwrap(), Some((pid, 1, 100)));
    assert_eq!(pidfd_info(&kernel, &parent, pidfd, 88, exit_asked), Ok(0));
    let info = pidfd_info_at(&parent);
    assert_eq!(
        info.mask,
        M::PIDFD_INFO_EXIT.0 | M::PIDFD_INFO_CGROUPID.0 | M::PIDFD_INFO_SUPPORTED_MASK.0
    );
    assert_eq!(info.exit_code, 100 << 8);
    assert_eq!(info.cgroupid, 1);
    assert_eq!(
        errno(pidfd_info(&kernel, &parent, pidfd, 88, 0)),
        Errno::ESRCH,
        "reaped, and the exit not asked for"
    );

    // One killed by a signal exits with it.
    let (child, pid) = fork(&kernel, &parent, 0).unwrap();
    let pidfd = block_on(kernel.pidfd_open(&parent.caller(), pid, 0)).unwrap();
    assert_eq!(kill(&kernel, &parent, pid, Signal::SIGKILL), Ok(0));
    assert!(dies(&kernel, &child));
    assert!(reap(&kernel, &parent, pid).unwrap().is_some());
    let exit = M::PIDFD_INFO_EXIT.0;
    assert_eq!(pidfd_info(&kernel, &parent, pidfd, 88, exit), Ok(0));
    assert_eq!(pidfd_info_at(&parent).exit_code, Signal::SIGKILL.0);
}

/// `reboot(LINUX_REBOOT_MAGIC1, LINUX_REBOOT_MAGIC2, cmd, NULL)`.
fn reboot(kernel: &Kernel, p: &P, cmd: wasixv2_abi::RebootCmd) -> SysResult<i32> {
    use wasixv2_abi::RebootMagic;
    let (magic1, magic2) = (
        RebootMagic::LINUX_REBOOT_MAGIC1,
        RebootMagic::LINUX_REBOOT_MAGIC2,
    );
    let null = wasixv2_abi::VoidPtr::NULL;
    block_on(kernel.reboot(&p.caller(), magic1.0 as i32, magic2.0 as i32, cmd, null))
}

/// `reboot_pid_ns`: in a child pid namespace, a restart, halt or power-off kills the
/// namespace's init, which its parent sees killed by SIGHUP or SIGINT; the machine goes on.
#[test]
fn reboot_in_a_pid_namespace_ends_it_with_the_signal_of_the_command() {
    use wasixv2_abi::RebootCmd;
    let commands = [
        (RebootCmd::LINUX_REBOOT_CMD_RESTART, Signal::SIGHUP),
        (RebootCmd::LINUX_REBOOT_CMD_RESTART2, Signal::SIGHUP),
        (RebootCmd::LINUX_REBOOT_CMD_HALT, Signal::SIGINT),
        (RebootCmd::LINUX_REBOOT_CMD_POWER_OFF, Signal::SIGINT),
    ];
    for (cmd, sig) in commands {
        let (kernel, parent) = machine();
        let (init, pid) = fork(&kernel, &parent, NEWPID).unwrap();
        let (child, _) = fork(&kernel, &init, 0).unwrap();
        // Only the commands that end the namespace.
        let cad = reboot(&kernel, &child, RebootCmd::LINUX_REBOOT_CMD_CAD_OFF);
        assert_eq!(errno(cad), Errno::EINVAL);
        assert!(kernel.ctrl_alt_del());

        assert_eq!(reboot(&kernel, &child, cmd), Ok(0));
        assert!(dies(&kernel, &init));
        assert!(dies(&kernel, &child));
        let killed = wasixv2_abi::SiCode::CLD_KILLED.0;
        assert_eq!(reap(&kernel, &parent, pid), Ok(Some((pid, killed, sig.0))));
        assert_eq!(kernel.power_request(), None);
        assert!(!dies(&kernel, &parent));
    }
}

/// In the initial pid namespace, `reboot` needs CAP_SYS_BOOT and the magic numbers; Ctrl-Alt-Del
/// toggles, and a power-off ends every process of the machine and is left for the embedder.
#[test]
fn reboot_in_the_initial_pid_namespace_powers_the_machine_off() {
    use wasixv2_abi::{RebootCmd, RebootMagic};
    let (kernel, task, memory) = super::cred::machine_with(|kernel| kernel.with_init());
    let user = Caller::new(task, &memory);
    let null = wasixv2_abi::VoidPtr::NULL;
    let (magic1, magic2) = (RebootMagic::LINUX_REBOOT_MAGIC1.0 as i32, 0x2812_1969);
    let off = RebootCmd::LINUX_REBOOT_CMD_POWER_OFF;
    assert_eq!(
        errno(block_on(kernel.reboot(&user, magic1, magic2, off, null))),
        Errno::EPERM
    );

    let (kernel, root) = machine();
    let caller = root.caller();
    let bad = block_on(kernel.reboot(&caller, magic1, 1, off, null));
    assert_eq!(errno(bad), Errno::EINVAL);
    for cmd in [
        RebootCmd::LINUX_REBOOT_CMD_KEXEC,
        RebootCmd::LINUX_REBOOT_CMD_SW_SUSPEND,
    ] {
        assert_eq!(errno(reboot(&kernel, &root, cmd)), Errno::EINVAL);
    }
    assert_eq!(
        reboot(&kernel, &root, RebootCmd::LINUX_REBOOT_CMD_CAD_OFF),
        Ok(0)
    );
    assert!(!kernel.ctrl_alt_del());
    assert_eq!(
        reboot(&kernel, &root, RebootCmd::LINUX_REBOOT_CMD_CAD_ON),
        Ok(0)
    );
    assert!(kernel.ctrl_alt_del());
    assert_eq!(kernel.power_request(), None);

    let (child, _) = fork(&kernel, &root, 0).unwrap();
    assert_eq!(reboot(&kernel, &child, off), Ok(0));
    assert_eq!(kernel.power_request(), Some(crate::PowerRequest::PowerOff));
    assert!(dies(&kernel, &child));
    assert!(dies(&kernel, &root));
}
