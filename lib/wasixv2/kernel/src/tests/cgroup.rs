//! Control groups v2: the `cgroup2` filesystem, migration and its rules, the `pids`
//! controller at fork, cgroup namespaces, `CLONE_INTO_CGROUP`, `/proc/<pid>/cgroup` and
//! `/proc/cgroups` (`kernel/cgroup/{cgroup,pids,namespace}.c`).

use wasixv2_abi::{
    CloneArgs, CloneArgsSize, CloneFlags, Idtype, PidfdInfo, PidfdInfoMask, SiginfoPtr, WaitOptions,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;

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
        Ok("memory pids\n".into())
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
            "#subsys_name\thierarchy\tnum_cgroups\tenabled\nmemory\t0\t1\t1\npids\t0\t1\t1\n"
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
            "nr_descendants 1\nnr_subsys_memory 2\nnr_subsys_pids 2\nnr_dying_descendants 0\n\
            nr_dying_subsys_memory 0\nnr_dying_subsys_pids 0\n"
                .into()
        )
    );
    assert_eq!(
        errno(p.write(&kernel, "/cg/cgroup.subtree_control", "+cpu")),
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
