//! Control groups v2: the `cgroup2` filesystem, migration and its rules, the `pids`
//! controller at fork, cgroup namespaces, `CLONE_INTO_CGROUP`, `/proc/<pid>/cgroup` and
//! `/proc/cgroups` (`kernel/cgroup/{cgroup,pids,namespace}.c`).

use wasixv2_abi::{
    CloneArgs, CloneArgsSize, CloneFlags, Idtype, PidfdInfo, PidfdInfoMask, RusagePtr, Siginfo,
    SiginfoPtr, WaitOptions,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;
use wasixv2_wait::Interrupt as _;

use super::{
    ipc::{errno, put},
    proc::ForkableMemory,
    *,
};

const CWD: Fd = Fd(-100);
const SIGCHLD: u32 = Signal::SIGCHLD.0 as u32;

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

    fn path(&self, path: &str) -> Cstr {
        let mut bytes = path.as_bytes().to_vec();
        bytes.push(0);
        put(&self.memory.0, 0x1000, &bytes);
        GuestPtr::new(0x1000)
    }

    fn open(&self, kernel: &Kernel, at: &str, flags: OpenFlags) -> SysResult<Fd> {
        let name = self.path(at);
        block_on(kernel.openat(&self.caller(), CWD, name, flags, 0))
    }

    fn read(&self, kernel: &Kernel, at: &str) -> SysResult<String> {
        let fd = self.open(kernel, at, OpenFlags::O_RDONLY)?;
        let mut out = Vec::new();
        loop {
            let buf = UserSliceMut::new(0x4000, 4096);
            let n = block_on(kernel.read(&self.caller(), fd, buf));
            let n = match n {
                Ok(n) => n,
                Err(err) => {
                    block_on(kernel.close(&self.caller(), fd)).unwrap();
                    return Err(err);
                }
            };
            if n == 0 {
                break;
            }
            out.extend(super::ipc::get(&self.memory.0, 0x4000, n as usize));
        }
        block_on(kernel.close(&self.caller(), fd)).unwrap();
        Ok(String::from_utf8(out).unwrap())
    }

    fn write(&self, kernel: &Kernel, at: &str, text: &str) -> SysResult<u32> {
        let fd = self.open(kernel, at, OpenFlags::O_WRONLY)?;
        put(&self.memory.0, 0x5000, text.as_bytes());
        let buf = UserSlice::new(0x5000, text.len() as u32);
        let written = block_on(kernel.write(&self.caller(), fd, buf));
        block_on(kernel.close(&self.caller(), fd)).unwrap();
        written
    }

    fn mkdir(&self, kernel: &Kernel, at: &str) -> SysResult<i32> {
        let name = self.path(at);
        block_on(kernel.mkdirat(&self.caller(), CWD, name, 0o755))
    }

    fn rmdir(&self, kernel: &Kernel, at: &str) -> SysResult<i32> {
        let name = self.path(at);
        block_on(kernel.unlinkat(&self.caller(), CWD, name, AtFlags::AT_REMOVEDIR))
    }

    fn fork(&self, kernel: &Kernel, flags: u32) -> SysResult<(P, Pid)> {
        let null = GuestPtrMut::NULL;
        let pid = block_on(kernel.clone(&self.caller(), flags | SIGCHLD, 0, null, 0, null))?;
        Ok((P::new(self.forked(kernel)), pid))
    }

    /// The child the last fork left.
    fn forked(&self, kernel: &Kernel) -> TaskId {
        let mut child = None;
        while let Some(action) = kernel.take_pending(self.task) {
            if let GuestAction::Fork { child: task, .. } = action {
                child = Some(task);
            }
        }
        child.expect("fork left the child")
    }

    fn pid(&self, kernel: &Kernel) -> Pid {
        block_on(kernel.getpid(&self.caller())).unwrap()
    }

    /// The process leaves its memory and exits, as its execution layer would make it.
    fn exit(&self, kernel: &Kernel) {
        block_on(kernel.exit_group(&self.caller(), 0)).unwrap();
        block_on(kernel.exit_mm(&self.caller()));
        kernel.release(self.task);
    }
}

/// A kernel with its init, `/proc` and the hierarchy at `/cg`, and one process running as
/// `uid`.
fn machine_as(uid: u32) -> (Kernel, P) {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        kernel
            .with_credentials(Credentials::new(uid, uid))
            .with_init()
    });
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    block_on(vfs.mount(b"/cg", kernel.cgroupfs(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

fn machine() -> (Kernel, P) {
    machine_as(0)
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// The root has the core files and those of the controllers it may give its children; a new
/// cgroup only the core ones until its parent enables a controller for it. Every process
/// starts in the root, and `/proc/<pid>/cgroup` says so once the hierarchy was mounted.
#[test]
fn the_root_lists_the_controllers_and_holds_every_process() {
    let (kernel, p) = machine();

    assert_eq!(
        p.read(&kernel, "/cg/cgroup.controllers"),
        Ok("cpu io memory pids\n".into())
    );
    assert_eq!(
        p.read(&kernel, "/cg/cgroup.subtree_control"),
        Ok(String::new())
    );
    let pid = p.pid(&kernel);
    let procs = p.read(&kernel, "/cg/cgroup.procs").unwrap();
    assert!(lines(&procs).contains(&pid.to_string().as_str()), "{procs}");
    assert!(lines(&procs).contains(&"1"), "init: {procs}");
    assert_eq!(p.read(&kernel, "/proc/self/cgroup"), Ok("0::/\n".into()));
    assert_eq!(errno(p.read(&kernel, "/cg/cgroup.events")), Errno::ENOENT);
    assert_eq!(errno(p.read(&kernel, "/cg/pids.max")), Errno::ENOENT);
    assert_eq!(
        p.read(&kernel, "/proc/cgroups"),
        Ok(
            "#subsys_name\thierarchy\tnum_cgroups\tenabled\ncpu\t0\t1\t1\nblkio\t0\t1\t1\n\
             memory\t0\t1\t1\npids\t0\t1\t1\n"
                .into()
        )
    );

    assert_eq!(p.mkdir(&kernel, "/cg/a"), Ok(0));
    assert_eq!(p.read(&kernel, "/cg/a/cgroup.type"), Ok("domain\n".into()));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.controllers"),
        Ok(String::new())
    );
    assert_eq!(errno(p.read(&kernel, "/cg/a/pids.max")), Errno::ENOENT);
    assert_eq!(
        p.write(&kernel, "/cg/cgroup.subtree_control", "+pids +memory"),
        Ok(13)
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.controllers"),
        Ok("memory pids\n".into())
    );
    assert_eq!(p.read(&kernel, "/cg/a/pids.max"), Ok("max\n".into()));
    assert_eq!(p.read(&kernel, "/cg/a/memory.max"), Ok("max\n".into()));
    assert_eq!(
        p.read(&kernel, "/cg/cgroup.stat"),
        Ok(
            "nr_descendants 1\nnr_subsys_cpu 1\nnr_subsys_io 1\nnr_subsys_memory 2\n\
             nr_subsys_pids 2\nnr_dying_descendants 0\nnr_dying_subsys_cpu 0\n\
             nr_dying_subsys_io 0\nnr_dying_subsys_memory 0\nnr_dying_subsys_pids 0\n"
                .into()
        )
    );
    assert_eq!(
        errno(p.write(&kernel, "/cg/cgroup.subtree_control", "+cpuset")),
        Errno::EINVAL
    );
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.subtree_control", "pids")),
        Errno::EINVAL
    );
}

/// A process moves with `cgroup.procs`; the cgroup is then populated and counts it, its
/// `/proc/<pid>/cgroup` names it, and it cannot be removed (`EBUSY`) until the process left.
#[test]
fn processes_move_between_cgroups() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+pids")
        .unwrap();
    p.mkdir(&kernel, "/cg/a").unwrap();
    let pid = p.pid(&kernel).to_string();

    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 0\nfrozen 0\n".into())
    );
    assert_eq!(
        p.write(&kernel, "/cg/a/cgroup.procs", &pid),
        Ok(pid.len() as u32)
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.procs"),
        Ok(format!("{pid}\n"))
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.threads"),
        Ok(format!("{pid}\n"))
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 0\n".into())
    );
    assert_eq!(p.read(&kernel, "/cg/a/pids.current"), Ok("1\n".into()));
    assert_eq!(p.read(&kernel, "/proc/self/cgroup"), Ok("0::/a\n".into()));
    assert_eq!(errno(p.rmdir(&kernel, "/cg/a")), Errno::EBUSY);

    // A child starts in its parent's cgroup.
    let (child, child_pid) = p.fork(&kernel, 0).unwrap();
    assert_eq!(p.read(&kernel, "/cg/a/pids.current"), Ok("2\n".into()));
    assert_eq!(
        p.read(&kernel, &format!("/proc/{child_pid}/cgroup")),
        Ok("0::/a\n".into())
    );

    // "0" is the writer itself.
    assert_eq!(p.write(&kernel, "/cg/cgroup.procs", "0"), Ok(1));
    assert_eq!(child.write(&kernel, "/cg/cgroup.procs", "0"), Ok(1));
    assert_eq!(p.read(&kernel, "/cg/a/cgroup.procs"), Ok(String::new()));
    assert_eq!(p.read(&kernel, "/cg/a/pids.current"), Ok("0\n".into()));
    assert_eq!(p.read(&kernel, "/cg/a/pids.peak"), Ok("2\n".into()));
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.procs", "99999")),
        Errno::ESRCH
    );
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.procs", "-1")),
        Errno::EINVAL
    );
    assert_eq!(p.rmdir(&kernel, "/cg/a"), Ok(0));
    assert_eq!(errno(p.read(&kernel, "/cg/a/cgroup.procs")), Errno::ENOENT);
}

/// `pids.max` refuses a fork past it with `EAGAIN`, counted in `pids.events` of the cgroup
/// whose limit it hit and above, and locally where it hit; a limit above holds below too.
#[test]
fn the_pids_controller_limits_forks() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+pids")
        .unwrap();
    p.mkdir(&kernel, "/cg/a").unwrap();
    p.write(&kernel, "/cg/a/cgroup.subtree_control", "+pids")
        .unwrap();
    p.mkdir(&kernel, "/cg/a/b").unwrap();
    p.write(&kernel, "/cg/a/b/cgroup.procs", "0").unwrap();

    assert_eq!(p.write(&kernel, "/cg/a/pids.max", "2"), Ok(1));
    assert_eq!(p.read(&kernel, "/cg/a/pids.max"), Ok("2\n".into()));
    let (_child, _) = p.fork(&kernel, 0).unwrap();
    assert_eq!(errno(p.fork(&kernel, 0).map(|(_, pid)| pid)), Errno::EAGAIN);
    assert_eq!(p.read(&kernel, "/cg/a/pids.events"), Ok("max 1\n".into()));
    assert_eq!(
        p.read(&kernel, "/cg/a/pids.events.local"),
        Ok("max 1\n".into())
    );
    assert_eq!(p.read(&kernel, "/cg/a/b/pids.events"), Ok("max 0\n".into()));
    assert_eq!(p.read(&kernel, "/cg/a/b/pids.current"), Ok("2\n".into()));
    // The first refusal in a cgroup is logged (`pids_event`).
    let log = block_on(kernel.syslog(&p.caller(), 3, GuestPtrMut::new(0x4000), 4096)).unwrap();
    let log = String::from_utf8(super::ipc::get(&p.memory.0, 0x4000, log as usize)).unwrap();
    assert!(
        log.ends_with("] cgroup: fork rejected by pids controller in /a/b\n"),
        "{log}"
    );
    // Moving in is not limited (`pids_can_attach`).
    p.mkdir(&kernel, "/cg/c").unwrap();
    p.write(&kernel, "/cg/c/pids.max", "0").unwrap();
    assert!(p.write(&kernel, "/cg/c/cgroup.procs", "0").is_ok());
    assert_eq!(p.read(&kernel, "/cg/c/pids.current"), Ok("1\n".into()));
    assert_eq!(errno(p.fork(&kernel, 0).map(|(_, pid)| pid)), Errno::EAGAIN);

    assert_eq!(
        errno(p.write(&kernel, "/cg/a/pids.max", "-1")),
        Errno::EINVAL
    );
    assert_eq!(p.write(&kernel, "/cg/c/pids.max", "max"), Ok(3));
    assert!(p.fork(&kernel, 0).is_ok());
}

/// The no-internal-process rule: a cgroup that holds processes cannot give its children a
/// domain controller, and one that gave them one takes no processes (`EBUSY`); the root is
/// exempt.
#[test]
fn a_cgroup_holds_processes_or_controls_children() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+memory")
        .unwrap();
    p.mkdir(&kernel, "/cg/a").unwrap();
    p.mkdir(&kernel, "/cg/a/b").unwrap();
    p.write(&kernel, "/cg/a/cgroup.procs", "0").unwrap();

    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.subtree_control", "+memory")),
        Errno::EBUSY
    );
    p.write(&kernel, "/cg/a/b/cgroup.procs", "0").unwrap();
    assert_eq!(
        p.write(&kernel, "/cg/a/cgroup.subtree_control", "+memory"),
        Ok(7)
    );
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.procs", "0")),
        Errno::EBUSY
    );
    // Disabling one a child enabled further down is refused too.
    p.write(&kernel, "/cg/a/b/cgroup.procs", "0").unwrap();
    p.write(&kernel, "/cg/cgroup.procs", "0").unwrap();
    p.write(&kernel, "/cg/a/b/cgroup.subtree_control", "+memory")
        .unwrap();
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.subtree_control", "-memory")),
        Errno::EBUSY
    );
    // Only what the parent enabled can be enabled.
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.subtree_control", "+pids")),
        Errno::ENOENT
    );
}

/// `cgroup.max.depth` and `cgroup.max.descendants` bound `mkdir` (`EAGAIN`); a removed
/// cgroup counts as dying until nothing holds it.
#[test]
fn hierarchy_limits_bound_mkdir() {
    let (kernel, p) = machine();
    p.mkdir(&kernel, "/cg/a").unwrap();
    assert_eq!(p.write(&kernel, "/cg/a/cgroup.max.depth", "1"), Ok(1));
    assert_eq!(p.mkdir(&kernel, "/cg/a/b"), Ok(0));
    assert_eq!(errno(p.mkdir(&kernel, "/cg/a/b/c")), Errno::EAGAIN);
    assert_eq!(p.write(&kernel, "/cg/a/cgroup.max.descendants", "1"), Ok(1));
    assert_eq!(errno(p.mkdir(&kernel, "/cg/a/d")), Errno::EAGAIN);
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.max.descendants"),
        Ok("1\n".into())
    );
    assert_eq!(p.read(&kernel, "/cg/cgroup.max.depth"), Ok("max\n".into()));
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.max.depth", "-1")),
        Errno::ERANGE
    );
    assert_eq!(errno(p.mkdir(&kernel, "/cg/a/cgroup.procs")), Errno::EEXIST);
    assert_eq!(
        errno(p.rmdir(&kernel, "/cg/a/cgroup.procs")),
        Errno::ENOTDIR
    );
    assert_eq!(errno(p.rmdir(&kernel, "/cg/a")), Errno::EBUSY);
    assert_eq!(p.rmdir(&kernel, "/cg/a/b"), Ok(0));
    assert_eq!(p.rmdir(&kernel, "/cg/a"), Ok(0));
    let stat = p.read(&kernel, "/cg/cgroup.stat").unwrap();
    assert!(stat.contains("nr_descendants 0\n"), "{stat}");
    assert!(
        stat.contains("nr_dying_descendants 0\n"),
        "nothing holds them: {stat}"
    );
}

/// A cgroup removed while a zombie of it is not reaped is dying until it is, and the zombie's
/// `/proc/<pid>/cgroup` says the cgroup was deleted.
#[test]
fn a_removed_cgroup_is_dying_until_its_zombies_go() {
    let (kernel, p) = machine();
    p.mkdir(&kernel, "/cg/a").unwrap();
    let (child, pid) = p.fork(&kernel, 0).unwrap();
    p.write(&kernel, "/cg/a/cgroup.procs", &pid.to_string())
        .unwrap();
    child.exit(&kernel);

    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 0\nfrozen 0\n".into())
    );
    assert_eq!(p.rmdir(&kernel, "/cg/a"), Ok(0));
    let stat = p.read(&kernel, "/cg/cgroup.stat").unwrap();
    assert!(stat.contains("nr_dying_descendants 1\n"), "{stat}");
    assert_eq!(
        p.read(&kernel, &format!("/proc/{pid}/cgroup")),
        Ok("0::/a (deleted)\n".into())
    );
    let options = WaitOptions(WaitOptions::WEXITED.0);
    let caller = p.caller();
    let wait = kernel.waitid(
        &caller,
        Idtype::P_PID,
        pid,
        SiginfoPtr::new(0x300),
        options,
        RusagePtr::NULL,
    );
    assert_eq!(block_on(wait), Ok(0));
    let stat = p.read(&kernel, "/cg/cgroup.stat").unwrap();
    assert!(stat.contains("nr_dying_descendants 0\n"), "{stat}");
}

/// Delegation: a writer needs write access to `cgroup.procs` of the common ancestor of where
/// a process is and where it goes, so a user given a subtree moves its processes within it
/// and nowhere else.
#[test]
fn a_delegated_subtree_is_its_owners_to_rearrange() {
    let (kernel, root) = machine();
    root.mkdir(&kernel, "/cg/user").unwrap();
    root.mkdir(&kernel, "/cg/user/a").unwrap();
    root.mkdir(&kernel, "/cg/user/b").unwrap();
    let (user, user_pid) = root.fork(&kernel, 0).unwrap();
    root.write(&kernel, "/cg/user/a/cgroup.procs", &user_pid.to_string())
        .unwrap();
    for path in [
        "/cg/user",
        "/cg/user/cgroup.procs",
        "/cg/user/a/cgroup.procs",
        "/cg/user/b/cgroup.procs",
    ] {
        let name = root.path(path);
        assert_eq!(
            block_on(kernel.fchownat(&root.caller(), CWD, name, 1000, 1000, AtFlags(0))),
            Ok(0)
        );
    }
    let uid = 1000;
    block_on(kernel.setresgid(&user.caller(), uid, uid, uid)).unwrap();
    block_on(kernel.setresuid(&user.caller(), uid, uid, uid)).unwrap();

    assert_eq!(user.write(&kernel, "/cg/user/b/cgroup.procs", "0"), Ok(1));
    assert_eq!(
        user.read(&kernel, "/proc/self/cgroup"),
        Ok("0::/user/b\n".into())
    );
    assert_eq!(
        errno(user.write(&kernel, "/cg/cgroup.procs", "0")),
        Errno::EACCES
    );
    assert_eq!(user.mkdir(&kernel, "/cg/user/c"), Ok(0));
    assert_eq!(errno(user.mkdir(&kernel, "/cg/d")), Errno::EACCES);
}

/// A cgroup namespace is rooted at its maker's cgroup: paths are shown from there, with `..`
/// for what is above, and a mount from inside shows only the subtree.
#[test]
fn a_cgroup_namespace_roots_the_view() {
    let (kernel, p) = machine();
    p.mkdir(&kernel, "/cg/a").unwrap();
    p.mkdir(&kernel, "/cg/b").unwrap();
    let (other, other_pid) = p.fork(&kernel, 0).unwrap();
    other.write(&kernel, "/cg/b/cgroup.procs", "0").unwrap();
    p.write(&kernel, "/cg/a/cgroup.procs", "0").unwrap();

    let newcgroup = CloneFlags::CLONE_NEWCGROUP.0 as Ulong;
    assert_eq!(block_on(kernel.unshare(&p.caller(), newcgroup)), Ok(0));
    assert_eq!(p.read(&kernel, "/proc/self/cgroup"), Ok("0::/\n".into()));
    assert_eq!(
        p.read(&kernel, &format!("/proc/{other_pid}/cgroup")),
        Ok("0::/../b\n".into())
    );
    p.mkdir(&kernel, "/mnt").unwrap();
    let string = |addr: u32, text: &str| {
        put(&p.memory.0, addr, format!("{text}\0").as_bytes());
        GuestPtr::new(addr)
    };
    let (source, target, fstype) = (
        string(0x1100, "none"),
        string(0x1200, "/mnt"),
        string(0x1300, "cgroup2"),
    );
    let flags = wasixv2_abi::MountFlags(0);
    let caller = p.caller();
    let mount = kernel.mount(&caller, source, target, fstype, flags, GuestPtr::NULL);
    assert_eq!(block_on(mount), Ok(0));
    let pid = p.pid(&kernel);
    assert_eq!(p.read(&kernel, "/mnt/cgroup.procs"), Ok(format!("{pid}\n")));
    assert_eq!(p.read(&kernel, "/mnt/cgroup.type"), Ok("domain\n".into()));
    // `nsdelegate` from inside a namespace changes nothing.
    let (source, target) = (string(0x1100, "none"), string(0x1200, "/mnt"));
    let remount = wasixv2_abi::MountFlags::MS_REMOUNT;
    put(&p.memory.0, 0x1400, b"nsdelegate\0");
    let data = GuestPtr::new(0x1400);
    let mount = kernel.mount(&caller, source, target, GuestPtr::NULL, remount, data);
    assert_eq!(block_on(mount), Ok(0));
    assert!(!kernel.cgroups.lock().flags.nsdelegate);
    drop(other);
}

/// `CLONE_INTO_CGROUP` starts the child in the cgroup its descriptor names, checked as a
/// move there would be; `EBADF` for anything but a cgroup directory.
#[test]
fn clone_into_cgroup_starts_the_child_there() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+memory")
        .unwrap();
    p.mkdir(&kernel, "/cg/a").unwrap();
    p.mkdir(&kernel, "/cg/a/b").unwrap();
    p.write(&kernel, "/cg/a/cgroup.subtree_control", "+memory")
        .unwrap();
    let dir = p
        .open(
            &kernel,
            "/cg/a/b",
            OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY,
        )
        .unwrap();
    let busy = p
        .open(&kernel, "/cg/a", OpenFlags::O_PATH | OpenFlags::O_DIRECTORY)
        .unwrap();
    let args = |cgroup: Fd| CloneArgs {
        flags: CloneFlags::CLONE_INTO_CGROUP,
        exit_signal: u64::from(SIGCHLD),
        cgroup: cgroup.0 as u64,
        ..CloneArgs::default()
    };
    let clone3 = |args: &CloneArgs| {
        block_on(write_value(&p.memory, GuestPtrMut::new(0x8000), args)).unwrap();
        let size = CloneArgsSize::CLONE_ARGS_SIZE_VER2.0;
        block_on(kernel.clone3(&p.caller(), GuestPtr::new(0x8000), size))
    };

    let pid = clone3(&args(dir)).unwrap();
    p.forked(&kernel);
    assert_eq!(
        p.read(&kernel, &format!("/proc/{pid}/cgroup")),
        Ok("0::/a/b\n".into())
    );
    assert_eq!(p.read(&kernel, "/proc/self/cgroup"), Ok("0::/\n".into()));
    assert_eq!(
        errno(clone3(&args(busy))),
        Errno::EBUSY,
        "a's children have memory"
    );
    let file = p
        .open(&kernel, "/cg/a/cgroup.procs", OpenFlags::O_RDONLY)
        .unwrap();
    assert_eq!(errno(clone3(&args(file))), Errno::EBADF);
    assert_eq!(errno(clone3(&args(Fd(77)))), Errno::EBADF);
}

/// `cgroup.kill` kills every process of the subtree.
#[test]
fn cgroup_kill_kills_the_subtree() {
    let (kernel, p) = machine();
    p.mkdir(&kernel, "/cg/a").unwrap();
    p.mkdir(&kernel, "/cg/a/b").unwrap();
    let (child, pid) = p.fork(&kernel, 0).unwrap();
    p.write(&kernel, "/cg/a/b/cgroup.procs", &pid.to_string())
        .unwrap();

    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.kill", "2")),
        Errno::ERANGE
    );
    assert_eq!(p.write(&kernel, "/cg/a/cgroup.kill", "1"), Ok(1));
    assert!(matches!(
        kernel.take_pending(child.task),
        Some(GuestAction::Exit(_))
    ));
    assert_eq!(kernel.take_pending(p.task), None, "the writer is elsewhere");
    child.exit(&kernel);
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 0\nfrozen 0\n".into())
    );
}

/// `cpu.stat` adds up the time tasks were charged while in the subtree: a task that moves
/// takes only its later time along, and a task that is gone leaves its time behind.
#[test]
fn cpu_stat_counts_time_spent_in_the_subtree() {
    let (kernel, p) = machine();
    p.mkdir(&kernel, "/cg/a").unwrap();
    let (child, pid) = p.fork(&kernel, 0).unwrap();
    let ms = std::time::Duration::from_millis;
    let account = |task| wasixv2_syscalls::CpuTime::cpu_account(&kernel, task);
    account(child.task).charge(ms(5), ms(1));
    p.write(&kernel, "/cg/a/cgroup.procs", &pid.to_string())
        .unwrap();
    account(child.task).charge(ms(20), ms(10));

    let stat = "usage_usec 30000\nuser_usec 20000\nsystem_usec 10000\nnice_usec 0\n";
    assert_eq!(p.read(&kernel, "/cg/a/cpu.stat"), Ok(stat.into()));
    child.exit(&kernel);
    assert_eq!(p.read(&kernel, "/cg/a/cpu.stat"), Ok(stat.into()));
    let root = p.read(&kernel, "/cg/cpu.stat").unwrap();
    assert!(root.starts_with("usage_usec 36000\n"), "{root}");
}

/// The memory controller shows what the subtree's address spaces hold, and keeps its limits.
#[test]
fn memory_files_show_the_address_spaces() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+memory")
        .unwrap();
    p.mkdir(&kernel, "/cg/a").unwrap();
    assert_eq!(p.read(&kernel, "/cg/a/memory.current"), Ok("0\n".into()));
    assert_eq!(p.write(&kernel, "/cg/a/memory.max", "1M"), Ok(2));
    assert_eq!(p.read(&kernel, "/cg/a/memory.max"), Ok("1048576\n".into()));
    assert_eq!(p.write(&kernel, "/cg/a/memory.high", "max"), Ok(3));
    assert_eq!(p.read(&kernel, "/cg/a/memory.high"), Ok("max\n".into()));
    let events = p.read(&kernel, "/cg/a/memory.events").unwrap();
    assert!(
        events.starts_with("low 0\nhigh 0\nmax 0\noom 0\n"),
        "{events}"
    );
    let stat = p.read(&kernel, "/cg/a/memory.stat").unwrap();
    assert!(stat.starts_with("anon 0\nfile 0\n"), "{stat}");
    // Gone with the controller.
    p.write(&kernel, "/cg/cgroup.subtree_control", "-memory")
        .unwrap();
    assert_eq!(errno(p.read(&kernel, "/cg/a/memory.max")), Errno::ENOENT);
}

/// Threaded mode: a cgroup turned threaded joins its parent's domain, which becomes a thread
/// root; only threaded controllers work there.
#[test]
fn a_threaded_cgroup_joins_its_parents_domain() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+pids +memory")
        .unwrap();
    p.mkdir(&kernel, "/cg/a").unwrap();
    p.mkdir(&kernel, "/cg/a/t").unwrap();
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/t/cgroup.type", "domain")),
        Errno::EINVAL
    );
    assert_eq!(p.write(&kernel, "/cg/a/t/cgroup.type", "threaded"), Ok(8));
    assert_eq!(
        p.read(&kernel, "/cg/a/t/cgroup.type"),
        Ok("threaded\n".into())
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.type"),
        Ok("domain threaded\n".into())
    );
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.subtree_control", "+memory")),
        Errno::EOPNOTSUPP
    );
    assert_eq!(
        p.write(&kernel, "/cg/a/cgroup.subtree_control", "+pids"),
        Ok(5)
    );
    p.write(&kernel, "/cg/a/t/cgroup.procs", "0").unwrap();
    assert_eq!(
        errno(p.read(&kernel, "/cg/a/t/cgroup.procs")),
        Errno::EOPNOTSUPP
    );
    let pid = p.pid(&kernel).to_string() + "\n";
    assert_eq!(p.read(&kernel, "/cg/a/cgroup.procs"), Ok(pid.clone()));
    assert_eq!(p.read(&kernel, "/cg/a/t/cgroup.threads"), Ok(pid));
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/t/cgroup.kill", "1")),
        Errno::EOPNOTSUPP
    );
}

/// `PIDFD_GET_INFO` gives the id of the process's cgroup: the inode number of its directory.
#[test]
fn pidfd_info_tells_the_cgroup() {
    let (kernel, p) = machine();
    p.mkdir(&kernel, "/cg/a").unwrap();
    let (_child, pid) = p.fork(&kernel, 0).unwrap();
    p.write(&kernel, "/cg/a/cgroup.procs", &pid.to_string())
        .unwrap();
    let pidfd = block_on(kernel.pidfd_open(&p.caller(), pid, 0)).unwrap();
    let mask = PidfdInfoMask::PIDFD_INFO_CGROUPID.0;
    put(&p.memory.0, 0x500, &mask.to_le_bytes());
    block_on(kernel.ioctl(&p.caller(), pidfd, IoctlRequest::PIDFD_GET_INFO, 0x500)).unwrap();
    let info: PidfdInfo = block_on(read_value(&p.memory, GuestPtr::new(0x500))).unwrap();

    let name = p.path("/cg/a");
    let out = GuestPtrMut::new(0x3000);
    block_on(kernel.statx(&p.caller(), CWD, name, AtFlags(0), StatxMask(0x7ff), out)).unwrap();
    let statx: wasixv2_abi::Statx = block_on(read_value(&p.memory, GuestPtr::new(0x3000))).unwrap();
    assert_eq!(info.cgroupid, statx.stx_ino);
    assert!(info.mask & PidfdInfoMask::PIDFD_INFO_CGROUPID.0 != 0);
}

/// Whether `task` waits in the freezer or a stop: its stop wait does not end yet.
fn held(kernel: &Kernel, task: TaskId) -> bool {
    use futures::FutureExt as _;
    Box::pin(kernel.stop_wait(task)).now_or_never().is_none()
}

/// `waitid(P_PID, pid, WSTOPPED | WNOHANG)` by `p`: whether it reports anything.
fn reports_stopped(kernel: &Kernel, p: &P, pid: Pid) -> bool {
    let infop = SiginfoPtr::new(0x300);
    let options = WaitOptions(WaitOptions::WSTOPPED.0 | WaitOptions::WNOHANG.0);
    let caller = p.caller();
    let wait = kernel.waitid(
        &caller,
        Idtype::P_PID,
        pid,
        infop,
        options,
        RusagePtr::new(0),
    );
    assert_eq!(block_on(wait), Ok(0));
    let info: Siginfo = block_on(read_value(&p.memory, GuestPtr::new(0x300))).unwrap();
    info.si_signo != 0
}

/// A child of `p` in the new cgroup `/cg/<name>`.
fn child_in(kernel: &Kernel, p: &P, name: &str) -> (P, Pid) {
    p.mkdir(kernel, &format!("/cg/{name}")).unwrap();
    let (child, pid) = p.fork(kernel, 0).unwrap();
    p.write(
        kernel,
        &format!("/cg/{name}/cgroup.procs"),
        &pid.to_string(),
    )
    .unwrap();
    (child, pid)
}

/// `cgroup_freeze` and `do_freezer_trap`: a frozen task waits on its way back to the guest
/// until it is thawed; the cgroup reports `frozen 1` once every task is, and the parent sees
/// no stop, nor does `/proc`.
#[test]
fn a_frozen_cgroup_holds_its_tasks_until_thawed() {
    let (kernel, p) = machine();
    let (child, pid) = child_in(&kernel, &p, "a");
    assert_eq!(p.read(&kernel, "/cg/a/cgroup.freeze"), Ok("0\n".into()));

    assert_eq!(p.write(&kernel, "/cg/a/cgroup.freeze", "1"), Ok(1));
    assert_eq!(p.read(&kernel, "/cg/a/cgroup.freeze"), Ok("1\n".into()));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 0\n".into()),
        "the task has yet to reach the freezer"
    );
    assert!(
        kernel.task_signals(child.task).pending(),
        "it leaves its wait"
    );
    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));
    assert!(held(&kernel, child.task));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 1\n".into())
    );
    assert!(
        !reports_stopped(&kernel, &p, pid),
        "a frozen task is not stopped"
    );
    let status = p.read(&kernel, &format!("/proc/{pid}/status")).unwrap();
    assert!(status.contains("State:\tS (sleeping)\n"), "{status}");
    // A signal that does not kill waits for the thaw.
    kernel.kill(child.task, Signal::SIGWINCH).unwrap();
    assert!(held(&kernel, child.task));
    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));

    // A thaw is seen at once (`cgroup_do_freeze` clears `CGRP_FROZEN`).
    assert_eq!(p.write(&kernel, "/cg/a/cgroup.freeze", "0"), Ok(1));
    assert!(!held(&kernel, child.task));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 0\n".into())
    );
    assert_eq!(
        kernel.take_pending(child.task),
        None,
        "SIGWINCH is ignored now"
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 0\n".into())
    );
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cgroup.freeze", "2")),
        Errno::ERANGE
    );
    assert_eq!(errno(p.read(&kernel, "/cg/cgroup.freeze")), Errno::ENOENT);
}

/// A fatal signal ends the freeze of the task it kills (`get_signal`'s `fatal` with
/// `cgroup_leave_frozen(true)`): SIGKILL, or one whose default action kills, which
/// `complete_signal` makes a group exit as it is sent. The cgroup, empty, is frozen.
#[test]
fn fatal_signals_reach_a_frozen_task() {
    for (sig, status) in [(Signal::SIGKILL, 128 + 9), (Signal::SIGTERM, 128 + 15)] {
        fatal_signal_reaches_a_frozen_task(sig, status);
    }
}

fn fatal_signal_reaches_a_frozen_task(sig: Signal, status: i32) {
    let (kernel, p) = machine();
    let (child, _) = child_in(&kernel, &p, "a");
    p.write(&kernel, "/cg/a/cgroup.freeze", "1").unwrap();
    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));

    kernel.kill(child.task, sig).unwrap();

    assert!(!held(&kernel, child.task));
    assert_eq!(
        kernel.take_pending(child.task),
        Some(GuestAction::Exit(status))
    );
    block_on(kernel.exit_mm(&child.caller()));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 0\nfrozen 1\n".into())
    );
}

/// A cgroup made in a freezing one freezes, frozen at once while empty, though its own
/// `cgroup.freeze` stays 0; a parent is frozen once its descendants are; a task forked or
/// moved into a freezing cgroup freezes, one moved out thaws (`cgroup_post_fork`,
/// `cgroup_freezer_migrate_task`).
#[test]
fn freezing_is_inherited_by_new_cgroups_and_tasks() {
    let (kernel, p) = machine();
    let (child, child_pid) = child_in(&kernel, &p, "a");
    p.write(&kernel, "/cg/a/cgroup.freeze", "1").unwrap();
    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 1\n".into())
    );

    p.mkdir(&kernel, "/cg/a/b").unwrap();
    assert_eq!(p.read(&kernel, "/cg/a/b/cgroup.freeze"), Ok("0\n".into()));
    assert_eq!(
        p.read(&kernel, "/cg/a/b/cgroup.events"),
        Ok("populated 0\nfrozen 1\n".into())
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 1\n".into())
    );

    // Moved in: the parent's other child freezes in `b`, and `a` waits for it.
    let (other, other_pid) = p.fork(&kernel, 0).unwrap();
    p.write(&kernel, "/cg/a/b/cgroup.procs", &other_pid.to_string())
        .unwrap();
    assert_eq!(
        p.read(&kernel, "/cg/a/b/cgroup.events"),
        Ok("populated 1\nfrozen 0\n".into())
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 0\n".into())
    );
    assert_eq!(kernel.take_pending(other.task), Some(GuestAction::Stopped));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 1\n".into())
    );

    // Moved out: it thaws, and its old cgroup stays frozen without it.
    p.write(&kernel, "/cg/cgroup.procs", &other_pid.to_string())
        .unwrap();
    assert!(!held(&kernel, other.task));
    assert_eq!(kernel.take_pending(other.task), None);
    assert_eq!(
        p.read(&kernel, "/cg/a/b/cgroup.events"),
        Ok("populated 0\nfrozen 1\n".into())
    );

    // A thaw from above thaws the subtree.
    p.write(&kernel, "/cg/a/cgroup.freeze", "0").unwrap();
    assert_eq!(
        p.read(&kernel, "/cg/a/b/cgroup.events"),
        Ok("populated 0\nfrozen 0\n".into())
    );
    assert_eq!(kernel.take_pending(child.task), None);
    let _ = child_pid;

    // A child forked in a freezing cgroup freezes before it runs.
    p.write(&kernel, "/cg/a/cgroup.freeze", "1").unwrap();
    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));
    let null = GuestPtrMut::NULL;
    block_on(kernel.clone(&child.caller(), SIGCHLD, 0, null, 0, null)).unwrap();
    // The parent, frozen, is held again once it took its fork's work.
    let grandchild = loop {
        match kernel.take_pending(child.task) {
            Some(GuestAction::Fork { child, .. }) => break child,
            Some(GuestAction::Stopped) | None => panic!("the fork left no child"),
            Some(_) => {}
        }
    };
    assert_eq!(kernel.take_pending(grandchild), Some(GuestAction::Stopped));
    let stat_local = p.read(&kernel, "/cg/a/cgroup.stat.local").unwrap();
    assert!(stat_local.starts_with("frozen_usec "), "{stat_local}");
}

/// A stopped task counts as frozen: a cgroup of stopped tasks freezes at once, and a stopped
/// task stays stopped, reported to its parent, while its cgroup freezes.
#[test]
fn stopped_tasks_count_as_frozen() {
    let (kernel, p) = machine();
    let (child, pid) = child_in(&kernel, &p, "a");
    kernel.kill(child.task, Signal::SIGSTOP).unwrap();
    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));

    p.write(&kernel, "/cg/a/cgroup.freeze", "1").unwrap();

    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 1\n".into())
    );
    assert!(reports_stopped(&kernel, &p, pid));
    kernel.kill(child.task, Signal::SIGCONT).unwrap();
    assert!(held(&kernel, child.task), "the freezer holds it now");
    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));
    assert_eq!(
        p.read(&kernel, "/cg/a/cgroup.events"),
        Ok("populated 1\nfrozen 1\n".into())
    );
}

/// `cgroup_file_notify` (`kernfs_notify`, `kernfs_generic_poll`): `cgroup.events` polls
/// `POLLPRI | POLLERR` until read, and again when it changes; an inotify watch on it, and on its
/// directory, hears `IN_MODIFY`. A refused fork notifies `pids.events` likewise.
#[test]
fn event_files_notify_pollers_and_inotify() {
    use wasixv2_abi::{InotifyInitFlags, InotifyMask};
    const PRI: u16 = 0x2;
    const ERR: u16 = 0x8;
    // The inotify half is delivered from the timer driver, which needs a host that runs it.
    let host: Arc<dyn Host> = Arc::new(super::ptimer::RealHost(std::time::Instant::now()));
    let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 0, 0);
    let kernel = Kernel::new(host, Arc::new(root))
        .with_credentials(Credentials::new(0, 0))
        .with_init();
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, task, _) = spawn_on(kernel, stdin, &[], &[]);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/cg", kernel.cgroupfs(), MountFlags::default(), true)).unwrap();
    let p = P::new(task);
    p.write(&kernel, "/cg/cgroup.subtree_control", "+pids")
        .unwrap();
    p.mkdir(&kernel, "/cg/a").unwrap();
    let events = p
        .open(&kernel, "/cg/a/cgroup.events", OpenFlags::O_RDONLY)
        .unwrap();
    let pids_events = p
        .open(&kernel, "/cg/a/pids.events", OpenFlags::O_RDONLY)
        .unwrap();
    let poll = |fd: Fd| {
        let fds = [(fd.0, PRI)];
        super::ipc::ppoll(&kernel, &p.caller(), &p.memory.0, &fds, Some((0, 0))).1[0]
    };
    let read = |fd: Fd| {
        let mut n = 0;
        while let Ok(got @ 1..) =
            block_on(kernel.pread64(&p.caller(), fd, UserSliceMut::new(0x4000, 4096), n))
        {
            n += got as i64;
        }
    };
    assert_eq!(poll(events), PRI | ERR, "never read");
    read(events);
    assert_eq!(poll(events), 0);

    let inotify =
        block_on(kernel.inotify_init1(&p.caller(), InotifyInitFlags::IN_NONBLOCK)).unwrap();
    let watch = |path: &str| {
        let name = p.path(path);
        block_on(kernel.inotify_add_watch(&p.caller(), inotify, name, InotifyMask::IN_MODIFY))
            .unwrap()
    };
    let file_wd = watch("/cg/a/cgroup.events");
    let dir_wd = watch("/cg/a");

    let (child, pid) = p.fork(&kernel, 0).unwrap();
    p.write(&kernel, "/cg/a/cgroup.procs", &pid.to_string())
        .unwrap();

    assert_eq!(poll(events), PRI | ERR, "populated");
    read(events);
    assert_eq!(poll(events), 0);
    let modify = InotifyMask::IN_MODIFY.0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut seen: Vec<(i32, u32, String)> = Vec::new();
    let file_event = (file_wd, modify, String::new());
    while !seen.contains(&file_event) {
        assert!(
            std::time::Instant::now() < deadline,
            "no inotify event: {seen:?}"
        );
        let buf = UserSliceMut::new(0x6000, 256);
        let Ok(n) = block_on(kernel.read(&p.caller(), inotify, buf)) else {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        };
        let queued = super::ipc::get(&p.memory.0, 0x6000, n as usize);
        let mut at = 0;
        while at + 16 <= queued.len() {
            let word =
                |i: usize| u32::from_le_bytes(queued[at + i..at + i + 4].try_into().unwrap());
            let len = word(12) as usize;
            let name = &queued[at + 16..at + 16 + len];
            let name = String::from_utf8_lossy(name)
                .trim_end_matches('\0')
                .to_string();
            seen.push((word(0) as i32, word(4), name));
            at += 16 + len;
        }
    }
    // `kernfs_notify_workfn`: the directory hears of it first, under the file's name.
    let dir_event = (dir_wd, modify, "cgroup.events".to_string());
    let at = |event| seen.iter().position(|e| *e == event).expect("an event");
    assert!(at(dir_event) < at(file_event), "{seen:?}");

    // `pids_event`.
    read(pids_events);
    assert_eq!(poll(pids_events), 0);
    p.write(&kernel, "/cg/a/pids.max", "1").unwrap();
    let refused = child.fork(&kernel, 0).map(|(_, pid)| pid);
    assert_eq!(errno(refused), Errno::EAGAIN);
    assert_eq!(poll(pids_events), PRI | ERR);
}

/// The `cpu` controller: `cpu.weight` and `cpu.weight.nice` are two views of one weight;
/// `cpu.max` throttles the subtree once its tasks used the quota of a period, until the period
/// ends (`throttle_cfs_rq`), which `cpu.stat`, `cpu.stat.local` and `cpu.pressure` count.
#[test]
fn cpu_max_throttles_the_subtree() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+cpu")
        .unwrap();
    let (child, _) = child_in(&kernel, &p, "a");
    assert_eq!(p.read(&kernel, "/cg/a/cpu.weight"), Ok("100\n".into()));
    assert_eq!(p.write(&kernel, "/cg/a/cpu.weight.nice", "-20"), Ok(3));
    assert_eq!(p.read(&kernel, "/cg/a/cpu.weight"), Ok("8668\n".into()));
    assert_eq!(p.read(&kernel, "/cg/a/cpu.weight.nice"), Ok("-20\n".into()));
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cpu.weight", "0")),
        Errno::ERANGE
    );
    assert_eq!(p.read(&kernel, "/cg/a/cpu.max"), Ok("max 100000\n".into()));
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cpu.max", "10")),
        Errno::EINVAL
    );
    assert_eq!(errno(p.read(&kernel, "/cg/cpu.max")), Errno::ENOENT);

    p.write(&kernel, "/cg/a/cpu.max", "1000 50000").unwrap();
    assert_eq!(p.read(&kernel, "/cg/a/cpu.max"), Ok("1000 50000\n".into()));
    let ms = std::time::Duration::from_millis;
    let account = wasixv2_syscalls::CpuTime::cpu_account(&kernel, child.task);
    account.charge(ms(2), ms(0));
    wasixv2_syscalls::CpuTime::cpu_timers(&kernel, child.task);

    assert_eq!(kernel.take_pending(child.task), Some(GuestAction::Stopped));
    let stat = p.read(&kernel, "/cg/a/cpu.stat").unwrap();
    assert!(
        stat.contains("nr_throttled 1\nthrottled_usec 0\n"),
        "{stat}"
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/cpu.stat.local"),
        Ok("throttled_usec 0\n".into())
    );
    let pressure = p.read(&kernel, "/cg/a/cpu.pressure").unwrap();
    assert!(pressure.starts_with("some avg10=0.00 "), "{pressure}");
    assert!(pressure.contains("\nfull "), "{pressure}");
    // The test host's clock stands still: the wait ends at once, and the task goes on.
    assert!(!held(&kernel, child.task));
    assert_eq!(kernel.take_pending(child.task), None);
    assert_eq!(
        errno(p.write(&kernel, "/cg/a/cpu.pressure", "some 150000 1000000")),
        Errno::EOPNOTSUPP
    );
}

/// The `io` controller: `io.stat` counts what the subtree read and wrote through files, by the
/// device of their filesystem; `io.max` is kept per device.
#[test]
fn io_stat_counts_file_reads_and_writes() {
    let (kernel, p) = machine();
    p.write(&kernel, "/cg/cgroup.subtree_control", "+io")
        .unwrap();
    let (child, _) = child_in(&kernel, &p, "a");
    assert_eq!(p.read(&kernel, "/cg/a/io.stat"), Ok(String::new()));
    let fd = child
        .open(&kernel, "/file", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    put(&child.memory.0, 0x5000, b"hello");
    let wrote = block_on(kernel.write(&child.caller(), fd, UserSlice::new(0x5000, 5)));
    assert_eq!(wrote, Ok(5));
    let read = block_on(kernel.pread64(&child.caller(), fd, UserSliceMut::new(0x4000, 64), 0));
    assert_eq!(read, Ok(5));

    let stat = p.read(&kernel, "/cg/a/io.stat").unwrap();
    assert!(
        stat.ends_with(" rbytes=5 wbytes=5 rios=1 wios=1 dbytes=0 dios=0\n"),
        "{stat}"
    );
    let root = p.read(&kernel, "/cg/io.stat").unwrap();
    assert!(
        root.contains(" rbytes=5 wbytes=5 "),
        "the subtree counts: {root}"
    );

    assert_eq!(p.write(&kernel, "/cg/a/io.max", "8:0 wbps=1048576"), Ok(16));
    assert_eq!(
        p.read(&kernel, "/cg/a/io.max"),
        Ok("8:0 rbps=max wbps=1048576 riops=max wiops=max\n".into())
    );
    assert_eq!(
        p.read(&kernel, "/cg/a/io.pressure"),
        Ok("some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n\
            full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"
            .into())
    );
}

#[test]
fn cgroup_migration_pids_use_the_openers_namespace_even_when_fd_is_shared() {
    let (kernel, root) = machine();
    root.mkdir(&kernel, "/cg/a").unwrap();
    root.mkdir(&kernel, "/cg/a/t").unwrap();
    root.write(&kernel, "/cg/a/t/cgroup.type", "threaded")
        .unwrap();
    let (opener, _) = root
        .fork(&kernel, CloneFlags::CLONE_NEWPID.0 as u32)
        .unwrap();
    assert_eq!(opener.pid(&kernel), 1);
    let (target, _) = opener.fork(&kernel, 0).unwrap();
    assert_eq!(target.pid(&kernel), 2);
    let share = |at| {
        let fd = opener.open(&kernel, at, OpenFlags::O_WRONLY).unwrap();
        let file = kernel.file(opener.task, fd).unwrap();
        let [slot] = kernel.reserve_fds::<1>(&root.caller(), false).unwrap();
        slot.install(file)
    };
    let write = |fd, text: &str| {
        put(&root.memory.0, 0x5000, text.as_bytes());
        block_on(kernel.write(
            &root.caller(),
            fd,
            UserSlice::new(0x5000, text.len() as u32),
        ))
    };
    let procs = share("/cg/a/cgroup.procs");
    assert_eq!(
        write(procs, "0x2"),
        Ok(3),
        "kstrtoint base 0 and the opener's pid 2"
    );
    let target_path = format!("/proc/{}/cgroup", target.task.0);
    assert_eq!(root.read(&kernel, &target_path).unwrap(), "0::/a\n");
    assert_eq!(
        root.read(&kernel, "/proc/self/cgroup").unwrap(),
        "0::/\n",
        "global pid 2 must not be migrated"
    );
    let threads = share("/cg/a/t/cgroup.threads");
    assert_eq!(write(threads, "02"), Ok(2));
    assert_eq!(root.read(&kernel, &target_path).unwrap(), "0::/a/t\n");
    assert_eq!(errno(write(procs, "9999")), Errno::ESRCH);
    assert_eq!(errno(write(procs, "2147483648")), Errno::ERANGE);
    assert_eq!(errno(write(procs, "-1")), Errno::EINVAL);
}
