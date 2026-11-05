//! User namespaces: making them, their id maps through `/proc`, ids as each namespace sees
//! them, and capabilities that count only over what a namespace owns.

use wasixv2_abi::{
    CloneFlags, ConstRlimit64Ptr, IoctlRequest, Rlimit64, Rlimit64Ptr, RlimitResource, Signal,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::MountFlags;

use super::{ipc::errno, proc::ForkableMemory, *};

const CWD: Fd = Fd(-100);
const SIGCHLD: u32 = Signal::SIGCHLD.0 as u32;
const NEWUSER: CloneFlags = CloneFlags::CLONE_NEWUSER;
/// `CapEff` of a process with every capability.
const ALL_CAPS: &str = "000001ffffffffff";

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

/// A kernel with its init and `/proc`, and one process running as `uid` and gid `uid`.
fn machine_as(uid: u32) -> (Kernel, P) {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        kernel
            .with_credentials(Credentials::new(uid, uid))
            .with_init()
    });
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

/// `clone` with `flags` and SIGCHLD: the child.
fn fork(kernel: &Kernel, parent: &P, flags: CloneFlags) -> SysResult<P> {
    let null = GuestPtrMut::NULL;
    let flags = flags.0 as u32 | SIGCHLD;
    block_on(kernel.clone(&parent.caller(), flags, 0, null, 0, null))?;
    let mut child = None;
    while let Some(action) = kernel.take_pending(parent.task) {
        if let GuestAction::Fork { child: task, .. } = action {
            child = Some(task);
        }
    }
    Ok(P::new(child.expect("fork left the child")))
}

fn unshare(kernel: &Kernel, p: &P, flags: CloneFlags) -> SysResult<i32> {
    block_on(kernel.unshare(&p.caller(), flags.0 as u32))
}

/// Puts `text` NUL-terminated at 0x1000.
fn path(p: &P, text: &str) -> Cstr {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    block_on(p.memory.write(0x1000, &bytes)).unwrap();
    GuestPtr::new(0x1000)
}

fn open(kernel: &Kernel, p: &P, at: &str, flags: OpenFlags) -> SysResult<Fd> {
    block_on(kernel.openat(&p.caller(), CWD, path(p, at), flags, 0))
}

/// The contents of the file `at`, read whole.
fn read_text(kernel: &Kernel, p: &P, at: &str) -> String {
    let caller = p.caller();
    let fd = open(kernel, p, at, OpenFlags::O_RDONLY).unwrap();
    let mut out = Vec::new();
    loop {
        let n = block_on(kernel.read(&caller, fd, UserSliceMut::new(0x5000, 4096))).unwrap();
        if n == 0 {
            break;
        }
        let mut chunk = vec![0; n as usize];
        block_on(p.memory.read(0x5000, &mut chunk)).unwrap();
        out.extend(chunk);
    }
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    String::from_utf8(out).unwrap()
}

/// Writes `text` to the file `at` in one write.
fn write_text(kernel: &Kernel, p: &P, at: &str, text: &str) -> SysResult<u32> {
    let caller = p.caller();
    let fd = open(kernel, p, at, OpenFlags::O_WRONLY)?;
    block_on(p.memory.write(0x5000, text.as_bytes())).unwrap();
    let written = block_on(kernel.write(&caller, fd, UserSlice::new(0x5000, text.len() as u32)));
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    written
}

/// The value of the line `key` of `/proc/self/status`.
fn status(kernel: &Kernel, p: &P, key: &str) -> String {
    let text = read_text(kernel, p, "/proc/self/status");
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}:\t")))
        .unwrap_or_else(|| panic!("no {key} in {text}"));
    line.to_string()
}

fn getuid(kernel: &Kernel, p: &P) -> i32 {
    block_on(kernel.getuid(&p.caller())).unwrap()
}

fn getgid(kernel: &Kernel, p: &P) -> i32 {
    block_on(kernel.getgid(&p.caller())).unwrap()
}

/// The pid of `p`, as it sees itself.
fn pid_of(kernel: &Kernel, p: &P) -> i32 {
    block_on(kernel.getpid(&p.caller())).unwrap()
}

/// `unshare(CLONE_NEWUSER)` and `0 <uid> 1` written to both maps, as `unshare -r` does:
/// `setgroups` denied first, so an unprivileged process may map its gid.
fn become_root(kernel: &Kernel, p: &P, extra: CloneFlags) {
    let (uid, gid) = (getuid(kernel, p), getgid(kernel, p));
    assert_eq!(unshare(kernel, p, NEWUSER | extra), Ok(0));
    let map = |id: i32| format!("0 {id} 1\n");
    assert_eq!(write_text(kernel, p, "/proc/self/setgroups", "deny"), Ok(4));
    let uid_map = map(uid);
    assert_eq!(
        write_text(kernel, p, "/proc/self/uid_map", &uid_map),
        Ok(uid_map.len() as u32)
    );
    let gid_map = map(gid);
    assert_eq!(
        write_text(kernel, p, "/proc/self/gid_map", &gid_map),
        Ok(gid_map.len() as u32)
    );
}

/// `NS_GET_OWNER_UID` of the caller's own user namespace.
fn owner_uid(kernel: &Kernel, p: &P) -> u32 {
    let caller = p.caller();
    let fd = open(kernel, p, "/proc/self/ns/user", OpenFlags::O_RDONLY).unwrap();
    let ioctl = kernel.ioctl(&caller, fd, IoctlRequest::NS_GET_OWNER_UID, 0x400);
    assert_eq!(block_on(ioctl), Ok(0));
    block_on(read_value(&p.memory, GuestPtr::<u32>::new(0x400))).unwrap()
}

/// LTP userns01: an unprivileged process in a new user namespace is the overflow uid and gid
/// with every capability, and the namespace's owner is itself, as it sees it once mapped.
#[test]
fn an_unprivileged_new_user_namespace_has_every_capability_and_no_ids_yet() {
    let (kernel, p) = machine_as(1000);
    assert_eq!(status(&kernel, &p, "CapEff"), "0000000000000000");

    assert_eq!(unshare(&kernel, &p, NEWUSER), Ok(0));

    assert_eq!((getuid(&kernel, &p), getgid(&kernel, &p)), (65534, 65534));
    assert_eq!(status(&kernel, &p, "Uid"), "65534\t65534\t65534\t65534");
    assert_eq!(status(&kernel, &p, "CapEff"), ALL_CAPS);
    assert_eq!(
        owner_uid(&kernel, &p),
        65534,
        "its creator is not mapped yet"
    );
    assert_eq!(read_text(&kernel, &p, "/proc/self/uid_map"), "");
    assert_eq!(read_text(&kernel, &p, "/proc/self/setgroups"), "allow\n");

    become_root_maps(&kernel, &p, 1000);
    assert_eq!((getuid(&kernel, &p), getgid(&kernel, &p)), (0, 0));
    assert_eq!(owner_uid(&kernel, &p), 0);
    assert_eq!(
        read_text(&kernel, &p, "/proc/self/uid_map"),
        "         0       1000          1\n"
    );
}

/// The maps of a namespace `p` already is in.
fn become_root_maps(kernel: &Kernel, p: &P, id: u32) {
    let map = format!("0 {id} 1");
    assert_eq!(
        write_text(kernel, p, "/proc/self/setgroups", "deny\n"),
        Ok(5)
    );
    assert_eq!(
        write_text(kernel, p, "/proc/self/uid_map", &map),
        Ok(map.len() as u32)
    );
    assert_eq!(
        write_text(kernel, p, "/proc/self/gid_map", &map),
        Ok(map.len() as u32)
    );
}

/// LTP userns05/06/07 rules for writing a map from inside: once, its own id only, never a
/// malformed or overlapping one, and the gid only after `setgroups` is denied.
#[test]
fn a_map_is_written_once_with_the_writers_own_ids() {
    let (kernel, p) = machine_as(1000);
    assert_eq!(unshare(&kernel, &p, NEWUSER), Ok(0));
    let write = |at: &str, text: &str| errno(write_text(&kernel, &p, at, text));

    assert_eq!(write("/proc/self/uid_map", "0 1001 1"), Errno::EPERM);
    assert_eq!(write("/proc/self/uid_map", "0 1000 2"), Errno::EPERM);
    assert_eq!(write("/proc/self/uid_map", "0 1000"), Errno::EINVAL);
    assert_eq!(write("/proc/self/uid_map", "0 1000 0"), Errno::EINVAL);
    assert_eq!(
        write("/proc/self/uid_map", "4294967295 1000 1"),
        Errno::EINVAL
    );
    assert_eq!(
        write("/proc/self/uid_map", "0 1000 1\n0 1001 1"),
        Errno::EINVAL,
        "overlapping"
    );
    assert_eq!(write("/proc/self/gid_map", "0 1000 1"), Errno::EPERM);
    assert_eq!(
        write_text(&kernel, &p, "/proc/self/uid_map", "0 1000 1"),
        Ok(8)
    );
    assert_eq!(write("/proc/self/uid_map", "0 1000 1"), Errno::EPERM);
    assert_eq!(write("/proc/self/setgroups", "maybe"), Errno::EINVAL);
    assert_eq!(
        write_text(&kernel, &p, "/proc/self/setgroups", "deny"),
        Ok(4)
    );
    assert_eq!(write("/proc/self/setgroups", "allow"), Errno::EPERM);
    assert_eq!(
        write_text(&kernel, &p, "/proc/self/gid_map", "0 1000 1"),
        Ok(8)
    );
    assert_eq!(
        write("/proc/self/setgroups", "deny"),
        Errno::EPERM,
        "after gid_map"
    );
    assert_eq!(read_text(&kernel, &p, "/proc/self/setgroups"), "deny\n");
    // setgroups(2) stays denied in the namespace, even for its root.
    let caller = p.caller();
    let groups = kernel.setgroups(&caller, 0, wasixv2_abi::ConstGidArray::new(0));
    assert_eq!(errno(block_on(groups)), Errno::EPERM);
}

/// A namespace made by `clone(CLONE_NEWUSER)`: its creator, root here, writes a range of
/// ids into the child's map from outside, as `newuidmap` does; each reader of the child's
/// map sees the lower ids in its own namespace.
#[test]
fn the_parent_maps_a_childs_range_and_each_level_sees_its_own_ids() {
    let (kernel, parent) = machine_as(0);
    let child = fork(&kernel, &parent, NEWUSER).unwrap();
    let at = |what: &str| format!("/proc/{}/{what}", pid_of(&kernel, &child));
    assert_eq!(getuid(&kernel, &child), 65534);

    for map in ["uid_map", "gid_map"] {
        let range = "0 100000 65536\n";
        assert_eq!(write_text(&kernel, &parent, &at(map), range), Ok(15));
    }

    // Root's kernel uid is not in the range: the child takes the namespace's root with the
    // capabilities it has there, and is the range's first id outside.
    assert_eq!(getuid(&kernel, &child), 65534);
    assert_eq!(block_on(kernel.setgid(&child.caller(), 0)), Ok(0));
    assert_eq!(block_on(kernel.setuid(&child.caller(), 0)), Ok(0));
    assert_eq!(getuid(&kernel, &child), 0);
    // Changing its ids made it non-dumpable, and its /proc files root's, as on Linux until
    // an `execve`.
    let dumpable = wasixv2_abi::PrctlOption::PR_SET_DUMPABLE;
    assert_eq!(
        block_on(kernel.prctl(&child.caller(), dumpable, 1, 0, 0, 0)),
        Ok(0)
    );
    let status_at = at("status");
    let seen = read_text(&kernel, &parent, &status_at);
    assert!(
        seen.contains("Uid:\t100000\t100000\t100000\t100000\n"),
        "{seen}"
    );
    assert_eq!(status(&kernel, &child, "CapEff"), ALL_CAPS);
    assert_eq!(
        read_text(&kernel, &parent, &at("uid_map")),
        "         0     100000      65536\n"
    );
    // The child's root makes a namespace of its own and maps part of its range.
    let grandchild = fork(&kernel, &child, NEWUSER).unwrap();
    let grand = format!("/proc/{}/uid_map", pid_of(&kernel, &grandchild));
    assert_eq!(write_text(&kernel, &child, &grand, "0 1000 10"), Ok(9));
    assert_eq!(
        read_text(&kernel, &child, &grand),
        "         0       1000         10\n"
    );
    assert_eq!(
        read_text(&kernel, &parent, &grand),
        "         0     101000         10\n"
    );
    assert_eq!(
        read_text(&kernel, &grandchild, "/proc/self/uid_map"),
        "         0       1000         10\n",
        "inside, the parent namespace's ids"
    );
    assert_eq!(block_on(kernel.setuid(&grandchild.caller(), 0)), Ok(0));
    assert_eq!(status(&kernel, &grandchild, "Uid"), "0\t0\t0\t0");
    let outside = format!("/proc/{}/status", pid_of(&kernel, &grandchild));
    let seen = read_text(&kernel, &parent, &outside);
    assert!(
        seen.contains("Uid:\t101000\t101000\t101000\t101000\n"),
        "{seen}"
    );
    assert_eq!(
        errno(write_text(
            &kernel,
            &parent,
            &grand.replace("uid", "gid"),
            "0 0 1"
        )),
        Errno::EPERM,
        "two levels up"
    );
}

/// `clone(CLONE_NEWUSER)` cannot share the root and working directory or the thread group.
#[test]
fn a_new_user_namespace_shares_neither_fs_nor_thread_group() {
    let (kernel, p) = machine_as(1000);
    let null = GuestPtrMut::NULL;
    for flags in [
        NEWUSER | CloneFlags::CLONE_FS,
        NEWUSER | CloneFlags::CLONE_THREAD | CloneFlags::CLONE_SIGHAND | CloneFlags::CLONE_VM,
    ] {
        let caller = p.caller();
        let clone = kernel.clone(&caller, flags.0 as u32, 0, null, 0, null);
        assert_eq!(errno(block_on(clone)), Errno::EINVAL, "{flags:?}");
    }
}

/// The root of a user namespace has every capability over what the namespace owns: a UTS
/// namespace made with it, not the one it came from.
#[test]
fn sethostname_needs_a_uts_namespace_the_user_namespace_owns() {
    let (kernel, p) = machine_as(1000);
    let sethostname = |name: &str| {
        block_on(p.memory.write(0x800, name.as_bytes())).unwrap();
        let name_ptr = wasixv2_abi::ConstBuf::new(0x800);
        block_on(kernel.sethostname(&p.caller(), name_ptr, name.len() as i32))
    };
    become_root(&kernel, &p, CloneFlags(0));
    assert_eq!(
        errno(sethostname("inside")),
        Errno::EPERM,
        "the initial one"
    );

    assert_eq!(unshare(&kernel, &p, CloneFlags::CLONE_NEWUTS), Ok(0));
    assert_eq!(sethostname("inside"), Ok(0));
    assert_eq!(
        read_text(&kernel, &p, "/proc/sys/kernel/hostname"),
        "inside\n"
    );
}

/// LTP userns04: setns into a user namespace needs `CAP_SYS_ADMIN` over it, which its creator
/// has and a sibling namespace's root does not.
#[test]
fn setns_enters_a_user_namespace_only_with_power_over_it() {
    let (kernel, parent) = machine_as(1000);
    let a = fork(&kernel, &parent, NEWUSER).unwrap();
    let b = fork(&kernel, &parent, NEWUSER).unwrap();
    let ns_of = |p: &P| format!("/proc/{}/ns/user", pid_of(&kernel, p));
    let b_ns = open(&kernel, &a, &ns_of(&b), OpenFlags::O_RDONLY).unwrap();
    let setns = |p: &P, fd: Fd| block_on(kernel.setns(&p.caller(), fd, NEWUSER.0 as i32));

    assert_eq!(
        errno(setns(&a, b_ns)),
        Errno::EPERM,
        "a sibling's namespace"
    );

    let a_ns = open(&kernel, &parent, &ns_of(&a), OpenFlags::O_RDONLY).unwrap();
    assert_eq!(status(&kernel, &parent, "CapEff"), "0000000000000000");
    assert_eq!(setns(&parent, a_ns), Ok(0), "the creator's");
    assert_eq!(status(&kernel, &parent, "CapEff"), ALL_CAPS);
    let own = open(&kernel, &parent, "/proc/self/ns/user", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(errno(setns(&parent, own)), Errno::EINVAL, "its own again");
}

/// `max_user_namespaces` bounds how many each user makes; 0 makes none (`ENOSPC`). It is a
/// limit of the reader's user namespace, which only `CAP_SYS_RESOURCE` there may change.
#[test]
fn max_user_namespaces_of_zero_makes_unshare_fail() {
    let (kernel, root) = machine_as(0);
    let at = "/proc/sys/user/max_user_namespaces";
    assert_eq!(read_text(&kernel, &root, at), "16384\n");
    let user = fork(&kernel, &root, CloneFlags(0)).unwrap();
    assert_eq!(block_on(kernel.setuid(&user.caller(), 1000)), Ok(0));
    assert_eq!(errno(write_text(&kernel, &user, at, "5")), Errno::EACCES);

    assert_eq!(write_text(&kernel, &root, at, "0\n"), Ok(2));

    assert_eq!(errno(unshare(&kernel, &user, NEWUSER)), Errno::ENOSPC);
    assert_eq!(
        fork(&kernel, &user, NEWUSER).err(),
        Some(Errno::ENOSPC.into())
    );
    assert_eq!(write_text(&kernel, &root, at, "1"), Ok(1));
    assert_eq!(unshare(&kernel, &user, NEWUSER), Ok(0));
    // The new namespace has a limit of its own, which its root may lower.
    assert_eq!(read_text(&kernel, &user, at), "2147483647\n");
    assert_eq!(write_text(&kernel, &user, at, "7"), Ok(1));
    assert_eq!(read_text(&kernel, &root, at), "1\n");
}

/// A chrooted process cannot make a user namespace (`current_chrooted`).
#[test]
fn a_chrooted_process_makes_no_user_namespace() {
    let (kernel, p) = machine_as(0);
    let caller = p.caller();
    let chroot = kernel.chroot(&caller, path(&p, "/proc"));
    assert_eq!(block_on(chroot), Ok(0));

    assert_eq!(errno(unshare(&kernel, &p, NEWUSER)), Errno::EPERM);
}

/// The negative set: root of a user namespace cannot raise a hard limit or write the initial
/// pid namespace's `pid_max`, which only the initial namespace's capabilities allow.
#[test]
fn a_user_namespaces_root_cannot_raise_limits_or_tune_the_machine() {
    let (kernel, p) = machine_as(1000);
    become_root(&kernel, &p, CloneFlags(0));
    let caller = p.caller();
    let limit = |new: Option<u64>| {
        let old = Rlimit64Ptr::new(0x600);
        let ptr = match new {
            Some(max) => {
                let value = Rlimit64 {
                    rlim_cur: 64,
                    rlim_max: max,
                };
                block_on(write_value(&p.memory, Rlimit64Ptr::new(0x700), &value)).unwrap();
                ConstRlimit64Ptr::new(0x700)
            }
            None => ConstRlimit64Ptr::NULL,
        };
        let res = RlimitResource::RLIMIT_NOFILE;
        block_on(kernel.prlimit64(&caller, 0, res, ptr, old))?;
        let old: Rlimit64 = block_on(read_value(&p.memory, GuestPtr::new(0x600))).unwrap();
        Ok::<_, SysErr>(old.rlim_max)
    };
    let max = limit(None).unwrap();

    assert_eq!(errno(limit(Some(max + 1))), Errno::EPERM);
    assert_eq!(limit(Some(max - 1)), Ok(max), "lowering needs nothing");
    let at = "/proc/sys/kernel/pid_max";
    assert_eq!(errno(write_text(&kernel, &p, at, "4096")), Errno::EACCES);
}

/// A tmpfs a user namespace's root may mount (`FS_USERNS_MOUNT`), or, as `hostfs`, one only the
/// initial namespace's root may, as a filesystem over the host's files.
struct TestFs {
    userns_mount: bool,
}

impl FilesystemType for TestFs {
    fn mount(
        &self,
        _source: &[u8],
        _params: &[FsParam<'_>],
        cred: &wasixv2_vfs::Cred,
    ) -> Result<Arc<dyn Filesystem>, Errno> {
        let clock = Arc::new(HostClock(Arc::new(TestHost) as Arc<dyn Host>));
        Ok(Arc::new(MemFs::new(clock, 0o1777, cred.fsuid, cred.fsgid)))
    }

    fn userns_mount(&self) -> bool {
        self.userns_mount
    }
}

/// A machine run as root with `tmpfs` and `hostfs` to mount, a sticky world-writable `/mnt`
/// whose `secret` only root may read, and a child running as uid and gid 1000.
fn mounting_machine() -> (Kernel, P, P) {
    let (kernel, root) = machine_as(0);
    for (name, userns_mount) in [("tmpfs", true), ("hostfs", false)] {
        let ty = Arc::new(TestFs { userns_mount });
        kernel.register_filesystem(name, ty).unwrap();
    }
    let clock = Arc::new(HostClock(Arc::new(TestHost) as Arc<dyn Host>));
    let mnt = Arc::new(MemFs::new(clock, 0o1777, 0, 0));
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/mnt", mnt, MountFlags::default(), true)).unwrap();
    let secret = open_mode(&kernel, &root, "/mnt/secret", 0o600);
    assert!(secret.is_ok());
    let user = fork(&kernel, &root, CloneFlags(0)).unwrap();
    assert_eq!(block_on(kernel.setgid(&user.caller(), 1000)), Ok(0));
    assert_eq!(block_on(kernel.setuid(&user.caller(), 1000)), Ok(0));
    // Dumpable again, as the `execve` after a login program's `setuid` makes it.
    let dumpable = wasixv2_abi::PrctlOption::PR_SET_DUMPABLE;
    assert_eq!(
        block_on(kernel.prctl(&user.caller(), dumpable, 1, 0, 0, 0)),
        Ok(0)
    );
    (kernel, root, user)
}

/// Creates the file `at` with `mode`.
fn open_mode(kernel: &Kernel, p: &P, at: &str, mode: u32) -> SysResult<Fd> {
    let flags = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
    block_on(kernel.openat(&p.caller(), CWD, path(p, at), flags, mode))
}

fn mount(kernel: &Kernel, p: &P, at: &str, fstype: &str) -> SysResult<i32> {
    block_on(p.memory.write(0x1800, b"none\0")).unwrap();
    let ty = format!("{fstype}\0");
    block_on(p.memory.write(0x1900, ty.as_bytes())).unwrap();
    let target = path(p, at);
    let none = wasixv2_abi::MountFlags(0);
    let caller = p.caller();
    let mount = kernel.mount(
        &caller,
        GuestPtr::new(0x1800),
        target,
        GuestPtr::new(0x1900),
        none,
        GuestPtr::NULL,
    );
    block_on(mount)
}

fn mkdir(kernel: &Kernel, p: &P, at: &str) -> SysResult<i32> {
    block_on(kernel.mkdirat(&p.caller(), CWD, path(p, at), 0o755))
}

/// The owner of `at` as `p` sees it, by `statx`.
fn owner(kernel: &Kernel, p: &P, at: &str) -> (u32, u32) {
    let out = GuestPtrMut::new(0x3000);
    let mask = wasixv2_abi::StatxMask(0x7ff);
    let caller = p.caller();
    let stat = kernel.statx(&caller, CWD, path(p, at), AtFlags(0), mask, out);
    assert_eq!(block_on(stat), Ok(0));
    let stat: wasixv2_abi::Statx = block_on(read_value(&p.memory, GuestPtr::new(0x3000))).unwrap();
    (stat.stx_uid, stat.stx_gid)
}

/// The root of a user namespace mounts a tmpfs in a mount namespace it owns, but nothing in
/// the initial mount namespace, and no filesystem that is not `FS_USERNS_MOUNT`.
#[test]
fn a_user_namespaces_root_mounts_a_tmpfs_in_a_mount_namespace_it_owns() {
    let (kernel, _root, user) = mounting_machine();
    assert_eq!(mkdir(&kernel, &user, "/mnt/t"), Ok(0));
    become_root(&kernel, &user, CloneFlags(0));
    assert_eq!(
        errno(mount(&kernel, &user, "/mnt/t", "tmpfs")),
        Errno::EPERM
    );

    assert_eq!(unshare(&kernel, &user, CloneFlags::CLONE_NEWNS), Ok(0));
    assert_eq!(mount(&kernel, &user, "/mnt/t", "tmpfs"), Ok(0));
    assert_eq!(
        owner(&kernel, &user, "/mnt/t"),
        (0, 0),
        "its root, as it sees itself"
    );
    assert_eq!(
        errno(mount(&kernel, &user, "/mnt/t", "hostfs")),
        Errno::EPERM
    );
}

/// What the root of a user namespace cannot do to files it has no mapping for, or to devices:
/// the capabilities apply over inodes whose owner its namespace maps.
#[test]
fn a_user_namespaces_root_has_no_power_over_unmapped_owners_or_devices() {
    let (kernel, root, user) = mounting_machine();
    become_root(&kernel, &user, CloneFlags::CLONE_NEWNS);

    assert_eq!(owner(&kernel, &user, "/mnt/secret"), (65534, 65534));
    let read = open(&kernel, &user, "/mnt/secret", OpenFlags::O_RDONLY);
    assert_eq!(
        errno(read),
        Errno::EACCES,
        "no CAP_DAC_OVERRIDE over root's file"
    );
    let chown = |at: &str, uid: u32| {
        let caller = user.caller();
        block_on(kernel.fchownat(&caller, CWD, path(&user, at), uid, u32::MAX, AtFlags(0)))
    };
    assert_eq!(errno(chown("/mnt/secret", 0)), Errno::EPERM, "not its file");
    assert_eq!(
        open_mode(&kernel, &user, "/mnt/mine", 0o644).map(|_| ()),
        Ok(())
    );
    assert_eq!(owner(&kernel, &user, "/mnt/mine"), (0, 0));
    assert_eq!(
        owner(&kernel, &root, "/mnt/mine"),
        (1000, 1000),
        "its kernel id outside"
    );
    assert_eq!(
        errno(chown("/mnt/mine", 5)),
        Errno::EINVAL,
        "an unmapped id"
    );
    assert_eq!(chown("/mnt/mine", 0), Ok(0));

    let mknod = |at: &str, mode: u32| {
        let caller = user.caller();
        block_on(kernel.mknodat(&caller, CWD, path(&user, at), mode, 0x0103))
    };
    assert_eq!(
        errno(mknod("/mnt/null", 0o020666)),
        Errno::EPERM,
        "a device"
    );
    assert_eq!(mknod("/mnt/fifo", 0o010600), Ok(0));
}

/// What is made on a filesystem is owned by ids its user namespace maps (`EOVERFLOW`): root,
/// unmapped in a user namespace's tmpfs, cannot create there.
#[test]
fn nothing_is_made_on_a_filesystem_whose_user_namespace_cannot_name_its_owner() {
    let (kernel, root, user) = mounting_machine();
    assert_eq!(mkdir(&kernel, &user, "/mnt/t"), Ok(0));
    become_root(&kernel, &user, CloneFlags::CLONE_NEWNS);
    assert_eq!(mount(&kernel, &user, "/mnt/t", "tmpfs"), Ok(0));
    let inside = format!("/proc/{}/root/mnt/t/made", pid_of(&kernel, &user));

    assert_eq!(
        errno(open_mode(&kernel, &root, &inside, 0o644)),
        Errno::EOVERFLOW
    );
    assert!(open_mode(&kernel, &user, "/mnt/t/made", 0o644).is_ok());
}

/// `mount` with `flags` and no type, for remounts.
fn remount(kernel: &Kernel, p: &P, at: &str, flags: u32) -> SysResult<i32> {
    let target = path(p, at);
    let caller = p.caller();
    let flags = wasixv2_abi::MountFlags(flags);
    let mount = kernel.mount(
        &caller,
        GuestPtr::NULL,
        target,
        GuestPtr::NULL,
        flags,
        GuestPtr::NULL,
    );
    block_on(mount)
}

fn umount(kernel: &Kernel, p: &P, at: &str) -> SysResult<i32> {
    let caller = p.caller();
    let flags = wasixv2_abi::UmountFlags(0);
    block_on(kernel.umount2(&caller, path(p, at), flags))
}

const MS_REMOUNT: u32 = wasixv2_abi::MountFlags::MS_REMOUNT.0;
const MS_BIND: u32 = wasixv2_abi::MountFlags::MS_BIND.0;
const MS_NOSUID: u32 = wasixv2_abi::MountFlags::MS_NOSUID.0;

/// LTP mountns, `namespace.c`'s `lock_mnt_tree`: the mounts a user namespace's mount namespace
/// gets from its parent stay over what they hide and keep the flags they have; what it mounts
/// itself is its own.
#[test]
fn a_mount_namespace_copied_for_a_user_namespace_has_its_mounts_locked() {
    let (kernel, _root, user) = mounting_machine();
    let clock = Arc::new(HostClock(Arc::new(TestHost) as Arc<dyn Host>));
    let srv = Arc::new(MemFs::new(clock, 0o1777, 0, 0));
    let nosuid = MountFlags {
        nosuid: true,
        ..MountFlags::default()
    };
    block_on(kernel.vfs().mount(b"/srv", srv, nosuid, true)).unwrap();
    assert_eq!(mkdir(&kernel, &user, "/mnt/t"), Ok(0));
    become_root(&kernel, &user, CloneFlags::CLONE_NEWNS);

    assert_eq!(errno(umount(&kernel, &user, "/srv")), Errno::EINVAL);
    assert_eq!(errno(umount(&kernel, &user, "/proc")), Errno::EINVAL);
    let rebind = MS_REMOUNT | MS_BIND;
    assert_eq!(
        errno(remount(&kernel, &user, "/srv", rebind)),
        Errno::EPERM,
        "clearing a locked nosuid"
    );
    assert_eq!(remount(&kernel, &user, "/srv", rebind | MS_NOSUID), Ok(0));
    assert_eq!(mount(&kernel, &user, "/mnt/t", "tmpfs"), Ok(0));
    assert_eq!(remount(&kernel, &user, "/mnt/t", rebind | MS_NOSUID), Ok(0));
    assert_eq!(
        remount(&kernel, &user, "/mnt/t", rebind),
        Ok(0),
        "its own mount"
    );
    assert_eq!(umount(&kernel, &user, "/mnt/t"), Ok(0));
}

/// procfs shows the whole machine: a user namespace's root mounts it in a pid namespace it
/// owns only where its mount namespace shows all of a procfs already, with nothing locked
/// on it (`mount_too_revealing`).
#[test]
fn procfs_mounts_in_a_user_namespace_only_where_one_is_fully_visible() {
    for covered in [false, true] {
        let (kernel, root, user) = mounting_machine();
        if covered {
            assert_eq!(mount(&kernel, &root, "/proc/sys", "tmpfs"), Ok(0));
        }
        become_root(
            &kernel,
            &user,
            CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID,
        );
        let init = fork(&kernel, &user, CloneFlags(0)).unwrap();

        let mounted = mount(&kernel, &init, "/proc", "proc");
        match covered {
            false => assert_eq!(mounted, Ok(0)),
            true => assert_eq!(errno(mounted), Errno::EPERM, "/proc/sys is covered"),
        }
        // Without a pid namespace of its own, procfs is the initial user namespace's.
        assert_eq!(errno(mount(&kernel, &user, "/proc", "proc")), Errno::EPERM);
    }
}

/// [`machine_as`] with a network whose loopback is up, and new network namespaces whose
/// loopback starts down.
fn networked_as(uid: u32) -> (Kernel, P) {
    let loopback = |up| {
        let config = wasixv2_vnet::NetConfig {
            loopback_up: up,
            ..wasixv2_vnet::NetConfig::default()
        };
        wasixv2_vnet::Net::new(config, None)
    };
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        kernel
            .with_network(loopback(true))
            .with_network_factory(move || loopback(false))
            .with_credentials(Credentials::new(uid, uid))
            .with_init()
    });
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

/// Interface requests and packet sockets ask `CAP_NET_ADMIN` and `CAP_NET_RAW` over the
/// owner of the network namespace (`dev_ioctl`, `devinet_ioctl`, `packet_create`:
/// `ns_capable(net->user_ns, ...)`): the root of a new user namespace has them in a network
/// namespace it made, not in the initial one.
#[test]
fn interface_requests_and_packet_sockets_ask_the_network_namespaces_owner() {
    use wasixv2_abi::AddressFamily;
    let (kernel, p) = networked_as(1000);
    become_root(&kernel, &p, CloneFlags(0));
    let caller = p.caller();
    let socket = |family, kind| block_on(kernel.socket(&caller, family, kind, 0));
    let set_lo_up = |fd| {
        block_on(p.memory.write(0x400, &[b'l', b'o', 0, 0])).unwrap();
        block_on(p.memory.write(0x410, &1i16.to_le_bytes())).unwrap();
        block_on(kernel.ioctl(&caller, fd, IoctlRequest::SIOCSIFFLAGS, 0x400))
    };
    let (dgram, raw) = (2, 3);

    let inet = socket(AddressFamily::AF_INET, dgram).unwrap();
    assert_eq!(
        errno(set_lo_up(inet)),
        Errno::EPERM,
        "the initial namespace's lo"
    );
    assert_eq!(errno(socket(AddressFamily::AF_PACKET, raw)), Errno::EPERM);

    assert_eq!(unshare(&kernel, &p, CloneFlags::CLONE_NEWNET), Ok(0));
    let inet = socket(AddressFamily::AF_INET, dgram).unwrap();
    assert_eq!(set_lo_up(inet), Ok(0), "its own namespace's lo");
    assert!(socket(AddressFamily::AF_PACKET, raw).is_ok());
}

/// `ping_group_range` holds kernel groups: the writer's user namespace maps what it writes in
/// (`make_kgid`, `EINVAL` for a group it does not map) and a reader's maps them out, so the
/// root of a user namespace that writes `0 0` lets its own group open ICMP datagram sockets
/// (`ipv4_ping_group_range`, `ping_init_sock`).
#[test]
fn ping_group_range_is_in_the_writers_user_namespace() {
    use wasixv2_abi::AddressFamily;
    let range = "/proc/sys/net/ipv4/ping_group_range";
    let (kernel, p) = networked_as(1000);
    become_root(&kernel, &p, CloneFlags::CLONE_NEWNET);
    let ping = || block_on(kernel.socket(&p.caller(), AddressFamily::AF_INET, 2, 1));
    assert_eq!(errno(ping()), Errno::EACCES, "no group by default");
    assert_eq!(
        write_text(&kernel, &p, range, "5 5"),
        Err(Errno::EINVAL.into()),
        "an unmapped group"
    );
    assert_eq!(write_text(&kernel, &p, range, "0 0"), Ok(3));
    assert_eq!(read_text(&kernel, &p, range), "0\t0\n");
    assert!(ping().is_ok());
}

/// sysfs shows the whole machine too (`FS_USERNS_MOUNT_RESTRICTED`): a user namespace's root
/// mounts it for a network namespace it owns only where a whole sysfs is visible already,
/// and not at all for the initial network namespace.
#[test]
fn sysfs_mounts_in_a_user_namespace_only_where_one_is_fully_visible() {
    for visible in [false, true] {
        let (kernel, root, user) = mounting_machine();
        assert_eq!(mkdir(&kernel, &root, "/mnt/sys"), Ok(0));
        if visible {
            assert_eq!(mount(&kernel, &root, "/mnt/sys", "sysfs"), Ok(0));
        }
        assert_eq!(mkdir(&kernel, &user, "/mnt/t"), Ok(0));
        become_root(&kernel, &user, CloneFlags::CLONE_NEWNS);
        assert_eq!(
            errno(mount(&kernel, &user, "/mnt/t", "sysfs")),
            Errno::EPERM,
            "the initial network namespace's"
        );
        assert_eq!(unshare(&kernel, &user, CloneFlags::CLONE_NEWNET), Ok(0));
        let mounted = mount(&kernel, &user, "/mnt/t", "sysfs");
        match visible {
            true => assert_eq!(mounted, Ok(0)),
            false => assert_eq!(errno(mounted), Errno::EPERM, "no sysfs shown whole"),
        }
    }
}

/// Dequeues a pending `sig` of `p` (`sigtimedwait` with a zero timeout), with its `si_uid`.
fn take_uid(kernel: &Kernel, p: &P, sig: Signal) -> Option<u32> {
    let mask = 1u64 << (sig.0 - 1);
    let these = wasixv2_abi::Sigset {
        sig: [mask as u32, (mask >> 32) as u32],
    };
    let now = wasixv2_abi::KernelTimespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    block_on(write_value(&p.memory, GuestPtrMut::new(0x170), &these)).unwrap();
    block_on(write_value(&p.memory, GuestPtrMut::new(0x178), &now)).unwrap();
    let caller = p.caller();
    let info = GuestPtrMut::new(0x200);
    let wait =
        kernel.rt_sigtimedwait_time64(&caller, GuestPtr::new(0x170), info, GuestPtr::new(0x178), 8);
    block_on(wait).ok()?;
    let info: wasixv2_abi::Siginfo = block_on(read_value(&p.memory, GuestPtr::new(0x200))).unwrap();
    Some(u32::from_le_bytes(info.sifields[4..8].try_into().unwrap()))
}

fn kill(kernel: &Kernel, p: &P, pid: i32, sig: Signal) -> SysResult<i32> {
    block_on(kernel.sys_kill(&p.caller(), pid, sig))
}

/// A signal's sender, a peer's credentials, an IPC object's owner and a child's uid in
/// `waitid` show as the receiver's user namespace sees them; an id it does not map is the
/// overflow id.
#[test]
fn ids_reach_a_user_namespace_as_it_sees_them() {
    let (kernel, outside) = machine_as(1000);
    let unix = wasixv2_abi::AddressFamily(1);
    let pair = wasixv2_abi::FdPair::new(0x100);
    let caller = outside.caller();
    assert_eq!(
        block_on(kernel.socketpair(&caller, unix, 1, 0, pair)),
        Ok(0)
    );
    let ipc_creat = wasixv2_abi::IpcFlags::IPC_CREAT.0 as i32;
    let msqid = block_on(kernel.msgget(&caller, 0, ipc_creat | 0o600)).unwrap();
    let inside = fork(&kernel, &outside, CloneFlags(0)).unwrap();
    become_root(&kernel, &inside, CloneFlags(0));
    let inside_pid = pid_of(&kernel, &inside);

    // SO_PEERCRED: the socket pair's maker, uid 1000, is root inside.
    let mut fd = [0u8; 4];
    block_on(outside.memory.read(0x100, &mut fd)).unwrap();
    let fd = Fd(i32::from_le_bytes(fd));
    block_on(write_value(
        &inside.memory,
        GuestPtrMut::<u32>::new(0x404),
        &12,
    ))
    .unwrap();
    let level = wasixv2_abi::SocketLevel::SOL_SOCKET;
    let peercred = wasixv2_abi::SocketOption::SO_PEERCRED.0;
    let caller = inside.caller();
    let get = kernel.getsockopt(
        &caller,
        fd,
        level,
        peercred,
        wasixv2_abi::Buf::new(0x400 + 0x10),
        wasixv2_abi::SocklenPtr::new(0x404),
    );
    assert_eq!(block_on(get), Ok(0));
    let mut ucred = [0u8; 12];
    block_on(inside.memory.read(0x410, &mut ucred)).unwrap();
    let word = |at: usize| u32::from_le_bytes(ucred[at..at + 4].try_into().unwrap());
    assert_eq!((word(4), word(8)), (0, 0));

    // IPC_STAT: the queue's owner and creator.
    let stat = kernel.msgctl(
        &caller,
        msqid,
        wasixv2_abi::IpcCmd::IPC_STAT.0,
        GuestPtrMut::new(0x600),
    );
    assert_eq!(block_on(stat), Ok(0));
    let ds: wasixv2_abi::Msqid64Ds =
        block_on(read_value(&inside.memory, GuestPtr::new(0x600))).unwrap();
    assert_eq!((ds.msg_perm.uid, ds.msg_perm.cuid), (0, 0));

    // A signal from uid 1000 comes from root inside, and back from uid 1000 outside.
    assert_eq!(kill(&kernel, &outside, inside_pid, Signal::SIGUSR1), Ok(0));
    assert_eq!(take_uid(&kernel, &inside, Signal::SIGUSR1), Some(0));
    let outside_pid = pid_of(&kernel, &outside);
    assert_eq!(kill(&kernel, &inside, outside_pid, Signal::SIGUSR2), Ok(0));
    assert_eq!(take_uid(&kernel, &outside, Signal::SIGUSR2), Some(1000));
}

/// `kill_ok_by_cred`: a user namespace's root has `CAP_KILL` over its own namespace only, so
/// it signals a process outside just as its uid could.
#[test]
fn a_user_namespaces_root_kills_only_what_its_uid_may() {
    let (kernel, root, user) = mounting_machine();
    let other = fork(&kernel, &root, CloneFlags(0)).unwrap();
    assert_eq!(block_on(kernel.setuid(&other.caller(), 2000)), Ok(0));
    let sibling = fork(&kernel, &user, CloneFlags(0)).unwrap();
    become_root(&kernel, &user, CloneFlags(0));
    let pid = |p: &P| pid_of(&kernel, p);

    assert_eq!(
        errno(kill(&kernel, &user, pid(&other), Signal::SIGUSR1)),
        Errno::EPERM
    );
    assert_eq!(
        kill(&kernel, &user, pid(&sibling), Signal::SIGUSR1),
        Ok(0),
        "uid 1000"
    );
    assert_eq!(
        errno(kill(&kernel, &user, pid(&root), Signal::SIGUSR1)),
        Errno::EPERM
    );
}

/// The rest of the negative set: a user namespace's root locks no memory past its
/// `RLIMIT_MEMLOCK` and fakes no input on a terminal that is not its own (`capable`, never a
/// user namespace's), where the initial namespace's root may.
#[test]
fn a_user_namespaces_root_neither_locks_memory_nor_fakes_terminal_input() {
    let (kernel, root, user) = mounting_machine();
    block_on(kernel.create_device_nodes(b"/mnt")).unwrap();
    let no_memlock = |p: &P| {
        let limit = Rlimit64 {
            rlim_cur: 0,
            rlim_max: 8 << 20,
        };
        block_on(write_value(&p.memory, Rlimit64Ptr::new(0x700), &limit)).unwrap();
        let caller = p.caller();
        let res = RlimitResource::RLIMIT_MEMLOCK;
        let set = kernel.prlimit64(
            &caller,
            0,
            res,
            ConstRlimit64Ptr::new(0x700),
            Rlimit64Ptr::NULL,
        );
        assert_eq!(block_on(set), Ok(0));
    };
    let mlock = |p: &P| {
        let caller = p.caller();
        let anon = wasixv2_abi::MapFlags(
            wasixv2_abi::MapFlags::MAP_PRIVATE.0 | wasixv2_abi::MapFlags::MAP_ANONYMOUS.0,
        );
        let rw = wasixv2_abi::ProtFlags(3);
        let at = block_on(kernel.mmap(&caller, 0, 4096, rw, anon, Fd(-1), 0)).unwrap();
        block_on(kernel.mlock(&caller, at, 4096))
    };
    let tiocsti = |p: &P| {
        let caller = p.caller();
        let master = open(
            &kernel,
            p,
            "/mnt/ptmx",
            OpenFlags::O_RDWR | OpenFlags::O_NOCTTY,
        );
        block_on(p.memory.write(0x30, b"x")).unwrap();
        block_on(kernel.ioctl(&caller, master.unwrap(), IoctlRequest::TIOCSTI, 0x30))
    };
    become_root(&kernel, &user, CloneFlags(0));
    no_memlock(&root);
    no_memlock(&user);

    assert_eq!(mlock(&root), Ok(0), "CAP_IPC_LOCK");
    assert_eq!(errno(mlock(&user)), Errno::EPERM);
    assert_eq!(tiocsti(&root), Ok(0), "CAP_SYS_ADMIN");
    assert_eq!(errno(tiocsti(&user)), Errno::EPERM);
}

/// `/proc/sys/net/ipv4/conf` of a network namespace that a user namespace owns is writable by
/// that namespace's root, which has `CAP_NET_ADMIN` over it (`net_ctl_permissions`), though the
/// files are root's of the initial namespace and `0644`; the process was not allowed before.
/// `forwarding` takes the capability itself (`devinet_sysctl_forward`): root of the initial
/// namespace without it opens the file and fails to write it with `EPERM`.
#[test]
fn a_network_namespaces_owner_writes_its_ipv4_conf() {
    let conf = "/proc/sys/net/ipv4/conf";
    let at = |name: &str| format!("{conf}/{name}");
    let (kernel, p) = networked_as(1000);
    assert_eq!(
        write_text(&kernel, &p, &at("lo/tag"), "1"),
        Err(Errno::EACCES.into())
    );
    become_root(&kernel, &p, CloneFlags::CLONE_NEWNET);
    assert_eq!(write_text(&kernel, &p, &at("lo/tag"), "7"), Ok(1));
    // The whole tree is the namespace's (`net_ctl_permissions` on every table).
    let dad = "/proc/sys/net/ipv6/conf/lo/accept_dad";
    assert_eq!(write_text(&kernel, &p, dad, "0"), Ok(1));
    assert_eq!(read_text(&kernel, &p, &at("lo/tag")), "7\n");
    assert_eq!(write_text(&kernel, &p, &at("all/forwarding"), "1"), Ok(1));
    assert_eq!(read_text(&kernel, &p, &at("lo/forwarding")), "1\n");
    assert_eq!(
        write_text(&kernel, &p, &at("all/mc_forwarding"), "1"),
        Err(Errno::EACCES.into())
    );

    // In a user namespace without a new network namespace, the initial one's is not its own.
    let (kernel, p) = networked_as(1000);
    become_root(&kernel, &p, CloneFlags::empty());
    assert_eq!(
        write_text(&kernel, &p, &at("lo/tag"), "1"),
        Err(Errno::EACCES.into())
    );
    assert_eq!(
        write_text(&kernel, &p, "/proc/sys/net/ipv6/conf/lo/accept_dad", "0"),
        Err(Errno::EACCES.into())
    );

    let (kernel, p) = machine_as(0);
    let all = u64::MAX >> (63 - u64::from(wasixv2_vfs::Cap::LAST));
    let net_admin = wasixv2_vfs::Cap::NetAdmin.bit();
    super::cred::set_caps(&kernel, &p.caller(), &p.memory.0, all & !net_admin);
    assert_eq!(
        write_text(&kernel, &p, &at("all/forwarding"), "1"),
        Err(Errno::EPERM.into())
    );
    assert_eq!(write_text(&kernel, &p, &at("all/tag"), "1"), Ok(1));
}

/// mqueue is `FS_USERNS_MOUNT` and belongs to the IPC namespace's owner
/// (`mqueue_init_fs_context`): a user namespace's root mounts the queues of an IPC namespace it
/// made, in a mount namespace it owns, but not the initial IPC namespace's. Its root is the
/// namespace maker's, so it shows as the namespace's root's.
#[test]
fn a_user_namespaces_root_mounts_the_queues_of_its_ipc_namespace() {
    let (kernel, _root, user) = mounting_machine();
    assert_eq!(mkdir(&kernel, &user, "/mnt/q"), Ok(0));
    become_root(&kernel, &user, CloneFlags::CLONE_NEWNS);
    assert_eq!(
        errno(mount(&kernel, &user, "/mnt/q", "mqueue")),
        Errno::EPERM
    );
    assert_eq!(unshare(&kernel, &user, CloneFlags::CLONE_NEWIPC), Ok(0));
    assert_eq!(mount(&kernel, &user, "/mnt/q", "mqueue"), Ok(0));
    assert_eq!(owner(&kernel, &user, "/mnt/q"), (0, 0));
    let fd = open_mode(&kernel, &user, "/mnt/q/queue", 0o600).unwrap();
    assert_eq!(block_on(kernel.close(&user.caller(), fd)), Ok(0));
    assert_eq!(owner(&kernel, &user, "/mnt/q/queue"), (0, 0));
}

/// `access(W_OK)` on the namespace sysctls answers as a write does: their table mode is 0644,
/// and `pid_table_root_permissions` and `set_permissions` give the owner's bits to the root of
/// the pid namespace's owner and to `CAP_SYS_RESOURCE` over the opener's user namespace, and
/// the others' (read-only) to everyone else.
#[test]
fn namespace_sysctls_are_writable_by_the_namespaces_root_only() {
    use wasixv2_abi::AccessMode;
    let access = |kernel: &Kernel, p: &P, at: &str, mode: AccessMode| {
        block_on(kernel.faccessat(&p.caller(), CWD, path(p, at), mode))
    };
    let (pid_max, max_user) = (
        "/proc/sys/kernel/pid_max",
        "/proc/sys/user/max_user_namespaces",
    );
    let (kernel, root) = machine_as(0);
    let user = fork(&kernel, &root, CloneFlags(0)).unwrap();
    assert_eq!(block_on(kernel.setuid(&user.caller(), 1000)), Ok(0));
    for at in [pid_max, max_user] {
        assert_eq!(access(&kernel, &root, at, AccessMode::W_OK), Ok(0), "{at}");
        assert_eq!(access(&kernel, &user, at, AccessMode::R_OK), Ok(0), "{at}");
        let denied = access(&kernel, &user, at, AccessMode::W_OK);
        assert_eq!(errno(denied), Errno::EACCES, "{at}");
    }

    // The root of a new user namespace: its own namespace limits, and the `pid_max` of the
    // pid namespace it made, but not the initial pid namespace's.
    let (kernel, user) = machine_as(1000);
    become_root(&kernel, &user, CloneFlags::CLONE_NEWPID);
    assert_eq!(access(&kernel, &user, max_user, AccessMode::W_OK), Ok(0));
    assert_eq!(write_text(&kernel, &user, max_user, "7"), Ok(1));
    let denied = access(&kernel, &user, pid_max, AccessMode::W_OK);
    assert_eq!(errno(denied), Errno::EACCES);
    let init = fork(&kernel, &user, CloneFlags(0)).unwrap();
    assert_eq!(access(&kernel, &init, pid_max, AccessMode::W_OK), Ok(0));
    assert_eq!(write_text(&kernel, &init, pid_max, "1000"), Ok(4));
}

/// Whether `execve` of a file named `name` in `p`'s user namespace would go to a binfmt_misc
/// interpreter (`current_binfmt_misc`, then the match).
fn misc_takes(kernel: &Kernel, p: &P, name: &[u8]) -> bool {
    let ns = kernel.task_cred(p.task).user_ns.clone();
    kernel.binfmt_misc.current(&ns).lookup(b"", name).is_some()
}

/// A user namespace that mounts binfmt_misc gets an instance of its own, owned by its root
/// (Linux 6.7): its formats apply to it and below, and the formats above stop applying to it.
/// Until then it uses its nearest ancestor's.
#[test]
fn binfmt_misc_has_an_instance_per_user_namespace_that_mounts_it() {
    let (kernel, root, user) = mounting_machine();
    assert_eq!(mkdir(&kernel, &root, "/mnt/bm"), Ok(0));
    assert_eq!(mount(&kernel, &root, "/mnt/bm", "binfmt_misc"), Ok(0));
    let init = ":init:E::ii::/prog:";
    assert_eq!(
        write_text(&kernel, &root, "/mnt/bm/register", init),
        Ok(init.len() as u32)
    );
    assert_eq!(mkdir(&kernel, &user, "/mnt/own"), Ok(0));
    become_root(&kernel, &user, CloneFlags::CLONE_NEWNS);
    assert!(misc_takes(&kernel, &user, b"/x.ii"), "the initial instance");

    assert_eq!(mount(&kernel, &user, "/mnt/own", "binfmt_misc"), Ok(0));
    assert_eq!(owner(&kernel, &user, "/mnt/own/register"), (0, 0));
    assert_eq!(
        errno(open(&kernel, &user, "/mnt/own/init", OpenFlags::O_RDONLY)),
        Errno::ENOENT
    );
    let own = ":own:E::oo::/prog:";
    assert_eq!(
        write_text(&kernel, &user, "/mnt/own/register", own),
        Ok(own.len() as u32)
    );
    assert!(misc_takes(&kernel, &user, b"/x.oo"));
    assert!(!misc_takes(&kernel, &user, b"/x.ii"));
    assert!(!misc_takes(&kernel, &root, b"/x.oo"));
    assert!(misc_takes(&kernel, &root, b"/x.ii"));
    assert_eq!(
        errno(open(&kernel, &root, "/mnt/bm/own", OpenFlags::O_RDONLY)),
        Errno::ENOENT
    );
}

#[test]
fn mount_namespace_owner_decodes_only_unobstructed_mapped_directory_subtrees() {
    use wasixv2_abi::{FileHandlePtr, S32Ptr};
    use wasixv2_vfs::Cap;
    let (kernel, root, user) = mounting_machine();
    for dir in [
        "/mnt/tree",
        "/mnt/tree/inside",
        "/mnt/tree/inside/child",
        "/mnt/tree/blocked",
        "/mnt/sibling",
    ] {
        assert_eq!(mkdir(&kernel, &user, dir), Ok(0));
    }
    // This mount becomes locked when copied into the less privileged namespace.
    assert_eq!(mount(&kernel, &root, "/mnt/tree/blocked", "tmpfs"), Ok(0));
    become_root(&kernel, &user, CloneFlags(0));
    let handle = |name: &str| {
        block_on(write_value(&user.memory, GuestPtrMut::new(0x3000), &128u32)).unwrap();
        let path = path(&user, name);
        assert_eq!(
            block_on(kernel.name_to_handle_at(
                &user.caller(),
                CWD,
                path,
                FileHandlePtr::new(0x3000),
                S32Ptr::new(0x3100),
                0
            )),
            Ok(0)
        );
    };
    let open_handle = |fd| {
        block_on(kernel.open_by_handle_at(
            &user.caller(),
            fd,
            GuestPtr::new(0x3000),
            OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY,
        ))
    };
    let anchor = open(&kernel, &user, "/mnt/tree/inside", OpenFlags::O_PATH).unwrap();
    handle("/mnt/tree/inside");
    assert_eq!(
        errno(open_handle(anchor)),
        Errno::EPERM,
        "does not own the initial mount namespace"
    );
    assert_eq!(unshare(&kernel, &user, CloneFlags::CLONE_NEWNS), Ok(0));
    let anchor = open(&kernel, &user, "/mnt/tree/inside", OpenFlags::O_PATH).unwrap();
    let _child = open(&kernel, &user, "/mnt/tree/inside/child", OpenFlags::O_PATH).unwrap();
    handle("/mnt/tree/inside/child");
    let decoded = open_handle(anchor).unwrap();
    block_on(kernel.close(&user.caller(), Fd(decoded))).unwrap();
    handle("/mnt/tree/inside");
    let decoded = open_handle(anchor).unwrap();
    block_on(kernel.close(&user.caller(), Fd(decoded))).unwrap();
    let _sibling = open(&kernel, &user, "/mnt/sibling", OpenFlags::O_PATH).unwrap();
    handle("/mnt/sibling");
    assert_eq!(
        errno(open_handle(anchor)),
        Errno::ESTALE,
        "cannot escape the supplied subtree"
    );
    handle("/mnt/tree/inside/child");
    let obstructed = open(&kernel, &user, "/mnt/tree", OpenFlags::O_PATH).unwrap();
    assert_eq!(
        errno(open_handle(obstructed)),
        Errno::EPERM,
        "locked child hides part of this subtree"
    );
    // Even the namespace owner still needs DAC_READ_SEARCH in its own user namespace.
    super::cred::set_caps(
        &kernel,
        &user.caller(),
        &user.memory.0,
        crate::cred::CAP_FULL & !Cap::DacReadSearch.bit(),
    );
    assert_eq!(errno(open_handle(anchor)), Errno::EPERM);
}
