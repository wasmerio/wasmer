//! Namespaces: `/proc/<pid>/ns`, nsfs files and their `NS_GET_*` requests.

use wasixv2_abi::{AddressFamily, CloneFlags, IoctlRequest, NewUtsname, Statx, StatxMask};
use wasixv2_syscalls::read_value;
use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, get, put},
    proc::ForkableMemory,
    *,
};

const CWD: Fd = Fd(-100);

/// A kernel with `/proc`, and one process running as `uid`.
fn machine_as(uid: u32) -> (Kernel, TaskId, ForkableMemory) {
    let (kernel, task, _) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(uid, uid)));
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
    (kernel, task, memory)
}

/// Puts `path` NUL-terminated at 0x1000 and returns it.
fn path(memory: &dyn UserMemory, path: &str) -> Cstr {
    let mut bytes = path.as_bytes().to_vec();
    bytes.push(0);
    block_on(memory.write(0x1000, &bytes)).unwrap();
    GuestPtr::new(0x1000)
}

fn open(kernel: &Kernel, caller: &Caller<'_>, at: &str) -> SysResult<Fd> {
    let name = path(caller.memory(), at);
    block_on(kernel.openat(caller, CWD, name, OpenFlags::O_RDONLY, 0))
}

fn readlink(kernel: &Kernel, caller: &Caller<'_>, at: &str) -> SysResult<String> {
    let name = path(caller.memory(), at);
    let n = block_on(kernel.readlinkat(caller, CWD, name, GuestPtrMut::new(0x2000), 256))?;
    let mut out = vec![0; n as usize];
    block_on(caller.memory().read(0x2000, &mut out)).unwrap();
    Ok(String::from_utf8(out).unwrap())
}

fn stat(kernel: &Kernel, caller: &Caller<'_>, at: &str) -> Statx {
    let name = path(caller.memory(), at);
    let out = GuestPtrMut::new(0x3000);
    let mask = StatxMask(0x7ff);
    block_on(kernel.statx(caller, CWD, name, AtFlags(0), mask, out)).unwrap();
    block_on(read_value(caller.memory(), GuestPtr::new(0x3000))).unwrap()
}

fn ioctl(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, request: IoctlRequest) -> SysResult<i32> {
    block_on(kernel.ioctl(caller, fd, request, 0x400))
}

/// The inode numbers of the initial namespaces (`linux/nsfs.h`).
const IPC_INO: u32 = 0xEFFF_FFFF;
const UTS_INO: u32 = 0xEFFF_FFFE;
const USER_INO: u32 = 0xEFFF_FFFD;
const PID_INO: u32 = 0xEFFF_FFFC;

#[test]
fn proc_ns_links_name_the_initial_namespaces() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);

    let expected = [
        ("net", "net", 0xEFFF_FFF9u32),
        ("uts", "uts", UTS_INO),
        ("ipc", "ipc", IPC_INO),
        ("pid", "pid", PID_INO),
        ("pid_for_children", "pid", PID_INO),
        ("user", "user", USER_INO),
        ("mnt", "mnt", 0xEFFF_FFF8),
        ("cgroup", "cgroup", 0xEFFF_FFFB),
        ("time", "time", 0xEFFF_FFFA),
        ("time_for_children", "time", 0xEFFF_FFFA),
    ];
    for (link, kind, ino) in expected {
        let at = format!("/proc/self/ns/{link}");
        assert_eq!(
            readlink(&kernel, &caller, &at),
            Ok(format!("{kind}:[{ino}]"))
        );
    }
    let own = format!("/proc/{}/ns/uts", task.0);
    assert_eq!(
        readlink(&kernel, &caller, &own),
        Ok(format!("uts:[{UTS_INO}]"))
    );
    assert_eq!(
        errno(readlink(&kernel, &caller, "/proc/self/ns/nope")),
        Errno::ENOENT
    );
}

#[test]
fn an_ns_file_is_its_namespaces_inode() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);

    let link = stat(&kernel, &caller, "/proc/self/ns/uts");
    let fd = open(&kernel, &caller, "/proc/self/ns/uts").unwrap();
    let again = open(&kernel, &caller, "/proc/thread-self/ns/uts").unwrap();
    let read = block_on(kernel.read(&caller, fd, UserSliceMut::new(0x200, 16)));

    assert_eq!(link.stx_ino, u64::from(UTS_INO));
    assert_eq!(
        u32::from(link.stx_mode),
        0o100444,
        "a regular file all may read"
    );
    assert_eq!(
        readlink(&kernel, &caller, &format!("/proc/self/fd/{}", fd.0)),
        Ok(format!("uts:[{UTS_INO}]"))
    );
    assert_eq!(
        readlink(&kernel, &caller, &format!("/proc/self/fd/{}", again.0)),
        Ok(format!("uts:[{UTS_INO}]"))
    );
    assert_eq!(errno(read), Errno::EINVAL, "an ns file has no data");
}

#[test]
fn ns_get_nstype_is_the_kinds_clone_flag() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);

    for (link, flag) in [
        ("uts", CloneFlags::CLONE_NEWUTS),
        ("user", CloneFlags::CLONE_NEWUSER),
        ("pid_for_children", CloneFlags::CLONE_NEWPID),
        ("mnt", CloneFlags::CLONE_NEWNS),
        ("time", CloneFlags::CLONE_NEWTIME),
    ] {
        let fd = open(&kernel, &caller, &format!("/proc/self/ns/{link}")).unwrap();
        let kind = ioctl(&kernel, &caller, fd, IoctlRequest::NS_GET_NSTYPE);
        assert_eq!(kind, Ok(flag.0 as i32), "{link}");
    }
}

#[test]
fn ns_get_parent_and_owner_uid_need_a_kind_that_has_them() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let uts = open(&kernel, &caller, "/proc/self/ns/uts").unwrap();
    let pid = open(&kernel, &caller, "/proc/self/ns/pid").unwrap();
    let user = open(&kernel, &caller, "/proc/self/ns/user").unwrap();
    put(&memory.0, 0x400, &[0xff; 4]);

    // No hierarchy for UTS namespaces; the initial pid and user namespaces have no parent.
    let uts_parent = ioctl(&kernel, &caller, uts, IoctlRequest::NS_GET_PARENT);
    let pid_parent = ioctl(&kernel, &caller, pid, IoctlRequest::NS_GET_PARENT);
    let user_parent = ioctl(&kernel, &caller, user, IoctlRequest::NS_GET_PARENT);
    let uts_owner = ioctl(&kernel, &caller, uts, IoctlRequest::NS_GET_OWNER_UID);
    let user_owner = ioctl(&kernel, &caller, user, IoctlRequest::NS_GET_OWNER_UID);

    assert_eq!(errno(uts_parent), Errno::EINVAL);
    assert_eq!(errno(pid_parent), Errno::EPERM);
    assert_eq!(errno(user_parent), Errno::EPERM);
    assert_eq!(errno(uts_owner), Errno::EINVAL);
    assert_eq!(user_owner, Ok(0));
    assert_eq!(
        get(&memory.0, 0x400, 4),
        [0; 4],
        "root made the initial one"
    );
}

#[test]
fn ns_get_userns_opens_the_owning_user_namespace() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let uts = open(&kernel, &caller, "/proc/self/ns/uts").unwrap();
    let user = open(&kernel, &caller, "/proc/self/ns/user").unwrap();

    let owner = Fd(ioctl(&kernel, &caller, uts, IoctlRequest::NS_GET_USERNS).unwrap());
    let none = ioctl(&kernel, &caller, user, IoctlRequest::NS_GET_USERNS);

    assert_eq!(
        readlink(&kernel, &caller, &format!("/proc/self/fd/{}", owner.0)),
        Ok(format!("user:[{USER_INO}]"))
    );
    assert!(kernel.files(task).lock_or_recover().cloexec(owner).unwrap());
    assert_eq!(
        ioctl(&kernel, &caller, owner, IoctlRequest::NS_GET_NSTYPE),
        Ok(CloneFlags::CLONE_NEWUSER.0 as i32)
    );
    assert_eq!(errno(none), Errno::EPERM, "the initial one has no owner");
}

#[test]
fn another_users_ns_links_are_not_for_reading() {
    let (kernel, root, memory) = machine_as(0);
    let other = kernel.spawn(ProcessConfig {
        args: vec!["other".into()],
        env: Vec::new(),
        stdio: Stdio {
            stdin: Arc::new(Input(Mutex::new(Vec::new()))),
            stdout: Arc::new(Capture::default()),
            stderr: Arc::new(Capture::default()),
        },
    });
    let caller = Caller::new(other, &memory);
    assert_eq!(block_on(kernel.setresuid(&caller, 1000, 1000, 1000)), Ok(0));

    let theirs = readlink(&kernel, &caller, &format!("/proc/{}/ns/uts", root.0));
    let mine = readlink(&kernel, &caller, "/proc/self/ns/uts");

    assert_eq!(errno(theirs), Errno::EACCES);
    assert_eq!(mine, Ok(format!("uts:[{UTS_INO}]")));
}

#[test]
fn a_child_shares_its_parents_namespaces() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let sigchld = Signal::SIGCHLD.0 as u32;
    let null = GuestPtrMut::NULL;

    let child = block_on(kernel.clone(&caller, sigchld, 0, null, 0, null)).unwrap();

    let (parent, child) = (kernel.task_ns(task), kernel.task_ns(TaskId(child as u32)));
    assert!(Arc::ptr_eq(&parent, &child));
}

const VFORK: u32 = CloneFlags::CLONE_VM.0 as u32 | CloneFlags::CLONE_VFORK.0 as u32;

/// vforks with `flags` besides `CLONE_VM | CLONE_VFORK`, and returns the child.
fn vfork(kernel: &Kernel, caller: &Caller<'_>, flags: u32) -> TaskId {
    let null = GuestPtrMut::NULL;
    let clone = kernel.clone(caller, VFORK | flags, 0, null, 0, null);
    assert_eq!(block_on(clone), Ok(0));
    match kernel.take_pending(caller.task()) {
        Some(GuestAction::SwitchTask { task, .. }) => task,
        other => panic!("vfork left {other:?}"),
    }
}

fn unshare(kernel: &Kernel, caller: &Caller<'_>, flags: CloneFlags) -> SysResult<i32> {
    block_on(kernel.unshare(caller, flags.0 as u32))
}

fn setns(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, flags: CloneFlags) -> SysResult<i32> {
    block_on(kernel.setns(caller, fd, flags.0 as i32))
}

#[test]
fn unshare_of_what_nothing_shares_succeeds() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let files = Arc::as_ptr(&kernel.files(task));

    for flags in [
        CloneFlags(0),
        CloneFlags::CLONE_FILES,
        CloneFlags::CLONE_FS,
        CloneFlags::CLONE_SYSVSEM,
        // Not implemented, and fine while the caller is alone.
        CloneFlags::CLONE_THREAD,
        CloneFlags::CLONE_SIGHAND,
        CloneFlags::CLONE_VM,
    ] {
        assert_eq!(unshare(&kernel, &caller, flags), Ok(0), "{flags:?}");
    }
    assert_eq!(files, Arc::as_ptr(&kernel.files(task)), "nothing to copy");
    assert_eq!(
        errno(unshare(&kernel, &caller, CloneFlags::CLONE_SETTLS)),
        Errno::EINVAL
    );
}

#[test]
fn unshare_files_gives_a_vfork_child_its_own_table() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let child = vfork(&kernel, &caller, CloneFlags::CLONE_FILES.0 as u32);
    let child_caller = Caller::new(child, &memory);
    assert!(Arc::ptr_eq(&kernel.files(task), &kernel.files(child)));

    assert_eq!(
        unshare(&kernel, &child_caller, CloneFlags::CLONE_FILES),
        Ok(0)
    );
    assert_eq!(block_on(kernel.close(&child_caller, Fd(0))), Ok(0));

    assert!(!Arc::ptr_eq(&kernel.files(task), &kernel.files(child)));
    assert!(kernel.file(task, Fd(0)).is_ok(), "the parent's stays open");
    // The child runs on its parent's memory, which it cannot stop sharing.
    assert_eq!(
        errno(unshare(&kernel, &child_caller, CloneFlags::CLONE_VM)),
        Errno::EINVAL
    );
}

#[test]
fn unshare_sysvsem_applies_the_semaphore_adjustments() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let ipc_creat = 0o600 | 0x200;
    let id = block_on(kernel.semget(&caller, 0, 1, ipc_creat)).unwrap();
    // `{ sem_num 0, sem_op +1, SEM_UNDO }`.
    put(&memory.0, 0x500, &[0, 0, 1, 0, 0x00, 0x10]);
    let op = block_on(kernel.semop(&caller, id, GuestPtr::new(0x500), 1));
    assert_eq!(op, Ok(0));
    let value = || block_on(kernel.semctl(&caller, id, 0, 12, 0));
    assert_eq!(value(), Ok(1));

    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_SYSVSEM), Ok(0));

    assert_eq!(value(), Ok(0), "undone as at exit");
}

#[test]
fn new_namespaces_need_cap_sys_admin() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let before = kernel.task_ns(task);

    for flags in [
        CloneFlags::CLONE_NEWUTS,
        CloneFlags::CLONE_NEWIPC,
        CloneFlags::CLONE_NEWNET,
        CloneFlags::CLONE_NEWNS,
        CloneFlags::CLONE_NEWPID,
    ] {
        assert_eq!(errno(unshare(&kernel, &caller, flags)), Errno::EPERM);
    }
    assert!(Arc::ptr_eq(&before, &kernel.task_ns(task)));
}

#[test]
fn setns_enters_the_namespace_of_an_ns_file() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let uts = open(&kernel, &caller, "/proc/self/ns/uts").unwrap();
    let before = kernel.task_ns(task);

    assert_eq!(setns(&kernel, &caller, uts, CloneFlags(0)), Ok(0));
    assert_eq!(
        setns(&kernel, &caller, uts, CloneFlags::CLONE_NEWUTS),
        Ok(0)
    );

    let after = kernel.task_ns(task);
    assert!(!Arc::ptr_eq(&before, &after), "a new proxy");
    assert!(Arc::ptr_eq(&before.uts, &after.uts), "the same namespace");
    assert_eq!(
        errno(setns(&kernel, &caller, uts, CloneFlags::CLONE_NEWNET)),
        Errno::EINVAL,
        "not the file's kind"
    );
}

#[test]
fn setns_needs_cap_sys_admin_and_an_ns_file_or_pidfd() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);
    let uts = open(&kernel, &caller, "/proc/self/ns/uts").unwrap();

    assert_eq!(
        errno(setns(&kernel, &caller, uts, CloneFlags(0))),
        Errno::EPERM
    );
    assert_eq!(
        errno(setns(&kernel, &caller, Fd(0), CloneFlags(0))),
        Errno::EINVAL,
        "not a namespace"
    );
    assert_eq!(
        errno(setns(&kernel, &caller, Fd(99), CloneFlags(0))),
        Errno::EBADF
    );
}

#[test]
fn setns_never_reenters_the_own_user_namespace() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let user = open(&kernel, &caller, "/proc/self/ns/user").unwrap();

    assert_eq!(
        errno(setns(&kernel, &caller, user, CloneFlags(0))),
        Errno::EINVAL
    );
}

#[test]
fn setns_through_a_pidfd_enters_the_processes_namespaces() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let pidfd = block_on(kernel.pidfd_open(&caller, task.0 as Pid, 0)).unwrap();
    let some = CloneFlags::CLONE_NEWUTS | CloneFlags::CLONE_NEWIPC | CloneFlags::CLONE_NEWNET;

    assert_eq!(setns(&kernel, &caller, pidfd, some), Ok(0));
    assert_eq!(
        errno(setns(&kernel, &caller, pidfd, CloneFlags(0))),
        Errno::EINVAL,
        "a pidfd needs the kinds"
    );
    assert_eq!(
        errno(setns(&kernel, &caller, pidfd, CloneFlags::CLONE_VM)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(setns(
            &kernel,
            &caller,
            pidfd,
            CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWUTS
        )),
        Errno::EINVAL,
        "the user namespace is the caller's own"
    );
}

#[test]
fn setns_into_a_mount_namespace_moves_to_its_root() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let dir = path(&memory, "/d");
    assert_eq!(block_on(kernel.mkdirat(&caller, CWD, dir, 0o755)), Ok(0));
    assert_eq!(block_on(kernel.chdir(&caller, dir)), Ok(0));
    let mnt = open(&kernel, &caller, "/proc/self/ns/mnt").unwrap();

    assert_eq!(setns(&kernel, &caller, mnt, CloneFlags(0)), Ok(0));

    let len = block_on(kernel.getcwd(&caller, UserSliceMut::new(0x600, 64)));
    assert_eq!(len, Ok(2));
    assert_eq!(get(&memory.0, 0x600, 2), b"/\0");
}

/// `PROC_DYNAMIC_FIRST`: the inode number of the first namespace made after boot.
const FIRST_DYNAMIC_INO: u32 = 0xF000_0000;

/// Forks the caller, and returns the child.
fn fork(kernel: &Kernel, caller: &Caller<'_>, flags: CloneFlags) -> TaskId {
    let null = GuestPtrMut::NULL;
    let flags = Signal::SIGCHLD.0 as u32 | flags.0 as u32;
    let child = block_on(kernel.clone(caller, flags, 0, null, 0, null)).unwrap();
    TaskId(child as u32)
}

fn sethostname(kernel: &Kernel, caller: &Caller<'_>, name: &str) -> SysResult<i32> {
    block_on(caller.memory().write(0x1800, name.as_bytes())).unwrap();
    block_on(kernel.sethostname(caller, GuestPtr::new(0x1800), name.len() as i32))
}

/// The caller's node name, as `uname` reports it.
fn nodename(kernel: &Kernel, caller: &Caller<'_>) -> String {
    let at = GuestPtrMut::new(0x4000);
    assert_eq!(block_on(kernel.uname(caller, at)), Ok(0));
    let names: NewUtsname = block_on(read_value(caller.memory(), GuestPtr::new(0x4000))).unwrap();
    let len = names.nodename.iter().position(|&b| b == 0).unwrap();
    String::from_utf8(names.nodename[..len].to_vec()).unwrap()
}

/// The inode number `/proc/self/ns/<link>` names.
fn ns_ino(kernel: &Kernel, caller: &Caller<'_>, link: &str) -> u32 {
    let target = readlink(kernel, caller, &format!("/proc/self/ns/{link}")).unwrap();
    let (_, ino) = target.split_once(":[").unwrap();
    ino.trim_end_matches(']').parse().unwrap()
}

#[test]
fn unshare_newuts_gives_the_caller_a_copy_of_the_names() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    assert_eq!(sethostname(&kernel, &caller, "before"), Ok(0));
    let child = fork(&kernel, &caller, CloneFlags(0));
    let child_caller = Caller::new(child, &memory);

    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWUTS), Ok(0));

    assert_eq!(nodename(&kernel, &caller), "before", "a copy");
    assert_eq!(ns_ino(&kernel, &caller, "uts"), FIRST_DYNAMIC_INO);
    assert_eq!(ns_ino(&kernel, &child_caller, "uts"), UTS_INO);
    assert_eq!(sethostname(&kernel, &caller, "mine"), Ok(0));
    assert_eq!(sethostname(&kernel, &child_caller, "theirs"), Ok(0));
    assert_eq!(nodename(&kernel, &caller), "mine");
    assert_eq!(nodename(&kernel, &child_caller), "theirs");
    let (ns, child_ns) = (kernel.task_ns(task), kernel.task_ns(child));
    assert!(
        Arc::ptr_eq(&ns.ipc, &child_ns.ipc),
        "only the UTS namespace"
    );
}

#[test]
fn clone_newuts_gives_the_child_its_own_names() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller, CloneFlags::CLONE_NEWUTS);
    let child_caller = Caller::new(child, &memory);

    assert_eq!(sethostname(&kernel, &child_caller, "LTP_HOSTNAME"), Ok(0));

    assert_eq!(nodename(&kernel, &child_caller), "LTP_HOSTNAME");
    assert_eq!(nodename(&kernel, &caller), "localhost");
    assert_eq!(ns_ino(&kernel, &child_caller, "uts"), FIRST_DYNAMIC_INO);
}

#[test]
fn namespace_inode_numbers_are_free_again_when_the_namespace_is_gone() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);

    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWUTS), Ok(0));
    let held = open(&kernel, &caller, "/proc/self/ns/uts").unwrap();
    let both = CloneFlags::CLONE_NEWUTS | CloneFlags::CLONE_NEWIPC;
    assert_eq!(unshare(&kernel, &caller, both), Ok(0));
    let (uts, ipc) = (
        ns_ino(&kernel, &caller, "uts"),
        ns_ino(&kernel, &caller, "ipc"),
    );
    assert_eq!((uts, ipc), (FIRST_DYNAMIC_INO + 1, FIRST_DYNAMIC_INO + 2));
    // The first UTS namespace lives on in the descriptor, the second goes with this unshare.
    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWUTS), Ok(0));
    assert_eq!(ns_ino(&kernel, &caller, "uts"), FIRST_DYNAMIC_INO + 3);
    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWUTS), Ok(0));
    assert_eq!(ns_ino(&kernel, &caller, "uts"), FIRST_DYNAMIC_INO + 1);

    assert_eq!(block_on(kernel.close(&caller, held)), Ok(0));
    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWUTS), Ok(0));
    assert_eq!(
        ns_ino(&kernel, &caller, "uts"),
        FIRST_DYNAMIC_INO,
        "the lowest free"
    );
}

#[test]
fn setns_enters_a_uts_namespace_another_process_made() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller, CloneFlags::CLONE_NEWUTS);
    let child_caller = Caller::new(child, &memory);
    assert_eq!(sethostname(&kernel, &child_caller, "inside"), Ok(0));
    let uts = open(&kernel, &caller, &format!("/proc/{}/ns/uts", child.0)).unwrap();

    assert_eq!(
        setns(&kernel, &caller, uts, CloneFlags::CLONE_NEWUTS),
        Ok(0)
    );

    assert_eq!(nodename(&kernel, &caller), "inside");
    assert_eq!(sethostname(&kernel, &caller, "shared"), Ok(0));
    assert_eq!(nodename(&kernel, &child_caller), "shared");
}

const IPC_CREAT: i32 = 0o1000;

/// `mq_open` of `name`, which libc passes without its leading slash.
fn mq_open(kernel: &Kernel, caller: &Caller<'_>, name: &str, flags: OpenFlags) -> SysResult<i32> {
    let name = path(caller.memory(), name);
    let flags = flags | OpenFlags::O_RDWR;
    block_on(kernel.mq_open(caller, name, flags, 0o600, GuestPtr::NULL))
}

#[test]
fn a_new_ipc_namespace_starts_without_objects() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let key = 0x1234;
    let sem = block_on(kernel.semget(&caller, key, 1, IPC_CREAT | 0o600)).unwrap();
    let msg = block_on(kernel.msgget(&caller, key, IPC_CREAT | 0o600)).unwrap();
    let shm = block_on(kernel.shmget(&caller, key, 4096, IPC_CREAT | 0o600)).unwrap();
    assert!(mq_open(&kernel, &caller, "q", OpenFlags::O_CREAT).is_ok());
    let child = fork(&kernel, &caller, CloneFlags(0));
    let child_caller = Caller::new(child, &memory);
    assert_eq!(block_on(kernel.semget(&child_caller, key, 0, 0)), Ok(sem));

    assert_eq!(
        unshare(&kernel, &child_caller, CloneFlags::CLONE_NEWIPC),
        Ok(0)
    );

    let in_child = |caller: &Caller<'_>| {
        [
            block_on(kernel.semget(caller, key, 0, 0)),
            block_on(kernel.msgget(caller, key, 0)),
            block_on(kernel.shmget(caller, key, 0, 0)),
            mq_open(&kernel, caller, "q", OpenFlags(0)),
        ]
        .map(errno)
    };
    assert_eq!(in_child(&child_caller), [Errno::ENOENT; 4]);
    // The child's objects are its namespace's, and the parent's stay.
    let theirs = block_on(kernel.msgget(&child_caller, key, IPC_CREAT | 0o600)).unwrap();
    assert_eq!(theirs, 0, "the first id of an empty namespace");
    assert_eq!(block_on(kernel.msgget(&caller, key, 0)), Ok(msg));
    assert_eq!(block_on(kernel.shmget(&caller, key, 0, 0)), Ok(shm));
    assert!(mq_open(&kernel, &caller, "q", OpenFlags(0)).is_ok());
}

#[test]
fn clone_newipc_cannot_share_semaphore_adjustments() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let null = GuestPtrMut::NULL;
    let flags = Signal::SIGCHLD.0 as u32
        | CloneFlags::CLONE_NEWIPC.0 as u32
        | CloneFlags::CLONE_SYSVSEM.0 as u32;

    let clone = block_on(kernel.clone(&caller, flags, 0, null, 0, null));

    assert_eq!(errno(clone), Errno::EINVAL);
}

/// The contents of the file `at`, read whole.
fn read_text(kernel: &Kernel, caller: &Caller<'_>, at: &str) -> String {
    let fd = open(kernel, caller, at).unwrap();
    let mut out = Vec::new();
    loop {
        let n = block_on(kernel.read(caller, fd, UserSliceMut::new(0x5000, 4096))).unwrap();
        if n == 0 {
            break;
        }
        let mut chunk = vec![0; n as usize];
        block_on(caller.memory().read(0x5000, &mut chunk)).unwrap();
        out.extend(chunk);
    }
    assert_eq!(block_on(kernel.close(caller, fd)), Ok(0));
    String::from_utf8(out).unwrap()
}

#[test]
fn proc_sys_kernel_shows_the_readers_uts_namespace() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller, CloneFlags::CLONE_NEWUTS);
    let child_caller = Caller::new(child, &memory);
    assert_eq!(sethostname(&kernel, &child_caller, "inside"), Ok(0));
    let domain = "lab";
    block_on(memory.write(0x1800, domain.as_bytes())).unwrap();
    let set = kernel.setdomainname(&child_caller, GuestPtr::new(0x1800), domain.len() as i32);
    assert_eq!(block_on(set), Ok(0));

    let theirs =
        |file: &str| read_text(&kernel, &child_caller, &format!("/proc/sys/kernel/{file}"));
    let mine = |file: &str| read_text(&kernel, &caller, &format!("/proc/sys/kernel/{file}"));

    assert_eq!(theirs("hostname"), "inside\n");
    assert_eq!(theirs("domainname"), "lab\n");
    assert_eq!(mine("hostname"), "localhost\n");
    assert_eq!(mine("domainname"), "(none)\n");
}

#[test]
fn proc_net_is_the_callers_own_net_directory() {
    let (kernel, task, memory) = machine_as(1);
    let caller = Caller::new(task, &memory);

    assert_eq!(
        readlink(&kernel, &caller, "/proc/net"),
        Ok("self/net".into())
    );
    let dev = read_text(&kernel, &caller, "/proc/net/dev");
    let own = read_text(&kernel, &caller, &format!("/proc/{}/net/dev", task.0));
    let thread = read_text(&kernel, &caller, "/proc/thread-self/net/tcp");

    assert!(dev.starts_with("Inter-|"), "{dev}");
    assert_eq!(own, dev);
    assert!(thread.starts_with("  sl  local_address"), "{thread}");
    assert_eq!(
        errno(open(&kernel, &caller, "/proc/self/net/nope")),
        Errno::ENOENT
    );
}

/// A kernel with `/proc`, an IP network in the initial namespace and a fresh loopback-only one
/// for each namespace made later, whose `lo` starts down as on Linux, and one process running
/// as root.
fn networked() -> (Kernel, TaskId, ForkableMemory) {
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
            .with_credentials(Credentials::new(0, 0))
    });
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (
        kernel,
        task,
        ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536]))),
    )
}

const STREAM: i32 = 1;
const DGRAM: i32 = 2;

/// Binds a new TCP socket to 127.0.0.1:`port` and listens on it.
fn listen_tcp(kernel: &Kernel, caller: &Caller<'_>, port: u16) -> SysResult<Fd> {
    let fd = block_on(kernel.socket(caller, AddressFamily::AF_INET, STREAM, 0))?;
    let addr = std::net::SocketAddr::new([127, 0, 0, 1].into(), port);
    block_on(caller.memory().write(0x700, &crate::socket::encode(addr))).unwrap();
    block_on(kernel.bind(caller, fd, GuestPtr::new(0x700), 16))?;
    block_on(kernel.listen(caller, fd, 4))?;
    Ok(fd)
}

#[test]
fn a_new_network_namespace_has_a_port_space_of_its_own() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    assert!(listen_tcp(&kernel, &caller, 8080).is_ok());
    let child = fork(&kernel, &caller, CloneFlags(0));
    let child_caller = Caller::new(child, &memory);

    assert_eq!(
        unshare(&kernel, &child_caller, CloneFlags::CLONE_NEWNET),
        Ok(0)
    );
    let socket = block_on(kernel.socket(&child_caller, AddressFamily::AF_INET, STREAM, 0));
    set_lo_flags(&kernel, &child_caller, socket.unwrap(), IFF_UP).unwrap();

    assert!(
        listen_tcp(&kernel, &child_caller, 8080).is_ok(),
        "free here"
    );
    assert_eq!(errno(listen_tcp(&kernel, &caller, 8080)), Errno::EADDRINUSE);
    assert_eq!(ns_ino(&kernel, &child_caller, "net"), FIRST_DYNAMIC_INO);
    let lo_only = read_text(&kernel, &child_caller, "/proc/net/dev");
    assert_eq!(lo_only.lines().skip(2).count(), 1, "{lo_only}");
    // Each task's /proc/<pid>/net shows its own namespace's sockets.
    let listening = |pid: TaskId| {
        let tcp = read_text(&kernel, &caller, &format!("/proc/{}/net/tcp", pid.0));
        tcp.lines().filter(|l| l.contains(":1F90 ")).count()
    };
    assert_eq!((listening(task), listening(child)), (1, 1));
}

/// `IFF_UP`, `IFF_LOOPBACK` and `IFF_RUNNING`.
const IFF_UP: i16 = 0x1;
const IFF_LOOPBACK: i16 = 0x8;
const IFF_RUNNING: i16 = 0x40;

/// `lo`'s flags through `SIOCGIFFLAGS` on `fd`.
fn lo_flags(kernel: &Kernel, caller: &Caller<'_>, fd: Fd) -> i16 {
    block_on(caller.memory().write(0x400, b"lo\0")).unwrap();
    ioctl(kernel, caller, fd, IoctlRequest::SIOCGIFFLAGS).unwrap();
    let mut flags = [0; 2];
    block_on(caller.memory().read(0x410, &mut flags)).unwrap();
    i16::from_le_bytes(flags)
}

/// `SIOCSIFFLAGS` of `lo` through `fd`, as `ip link set lo up` without netlink does.
fn set_lo_flags(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, flags: i16) -> SysResult<i32> {
    block_on(caller.memory().write(0x400, &[b'l', b'o', 0, 0])).unwrap();
    block_on(caller.memory().write(0x410, &flags.to_le_bytes())).unwrap();
    ioctl(kernel, caller, fd, IoctlRequest::SIOCSIFFLAGS)
}

/// A namespace a process makes starts with `lo` down and without addresses (`copy_net_ns`,
/// `loopback_net_init`), so nothing binds or reaches 127.0.0.1 until `SIOCSIFFLAGS` brings it
/// up, which gives it 127.0.0.1/8 and ::1.
#[test]
fn a_new_network_namespace_starts_with_loopback_down() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);

    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWNET), Ok(0));

    let fd = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM, 0)).unwrap();
    assert_eq!(lo_flags(&kernel, &caller, fd), IFF_LOOPBACK);
    assert_eq!(
        errno(listen_tcp(&kernel, &caller, 80)),
        Errno::EADDRNOTAVAIL
    );
    // On an AF_INET socket an alias stands for its address (`devinet_ioctl`), and lo has
    // none yet; IFF_LOOPBACK and IFF_RUNNING are the device's own.
    put(&memory.0, 0x400, b"lo:1\0");
    put(&memory.0, 0x410, &IFF_UP.to_le_bytes());
    assert_eq!(
        errno(ioctl(&kernel, &caller, fd, IoctlRequest::SIOCSIFFLAGS)),
        Errno::EADDRNOTAVAIL
    );
    assert_eq!(set_lo_flags(&kernel, &caller, fd, IFF_UP), Ok(0));
    assert_eq!(
        lo_flags(&kernel, &caller, fd),
        IFF_UP | IFF_LOOPBACK | IFF_RUNNING
    );
    assert!(listen_tcp(&kernel, &caller, 80).is_ok());
    assert_eq!(set_lo_flags(&kernel, &caller, fd, 0), Ok(0));
    assert_eq!(lo_flags(&kernel, &caller, fd), IFF_LOOPBACK);
    put(&memory.0, 0x400, b"eth9\0");
    assert_eq!(
        errno(ioctl(&kernel, &caller, fd, IoctlRequest::SIOCSIFFLAGS)),
        Errno::ENODEV
    );
}

/// `SIOCSIFFLAGS` needs `CAP_NET_ADMIN`, checked before the interface is looked up.
#[test]
fn changing_interface_flags_needs_cap_net_admin() {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        let loopback = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
        kernel
            .with_network(loopback)
            .with_credentials(Credentials::new(1, 1))
    });
    let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
    let caller = Caller::new(task, &memory);
    let fd = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM, 0)).unwrap();

    assert_eq!(errno(set_lo_flags(&kernel, &caller, fd, 0)), Errno::EPERM);
    put(&memory.0, 0x400, b"eth9\0");
    assert_eq!(
        errno(ioctl(&kernel, &caller, fd, IoctlRequest::SIOCSIFFLAGS)),
        Errno::EPERM
    );
    assert_eq!(lo_flags(&kernel, &caller, fd) & IFF_UP, IFF_UP);
}

#[test]
fn without_a_factory_a_new_network_namespace_has_no_ip_network() {
    let (kernel, task, memory) = machine_as(0);
    let caller = Caller::new(task, &memory);

    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWNET), Ok(0));

    let socket = block_on(kernel.socket(&caller, AddressFamily::AF_INET, STREAM, 0));
    assert_eq!(errno(socket), Errno::EAFNOSUPPORT);
    let unix = block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, STREAM, 0));
    assert!(unix.is_ok());
}

/// A `sockaddr_un` at 0x900 for `name` (abstract when it starts with NUL), and its length.
fn sun(memory: &dyn UserMemory, name: &[u8]) -> u32 {
    let mut addr = (AddressFamily::AF_UNIX.0 as u16).to_le_bytes().to_vec();
    addr.extend(name);
    block_on(memory.write(0x900, &addr)).unwrap();
    addr.len() as u32
}

fn unix_listen(kernel: &Kernel, caller: &Caller<'_>, name: &[u8]) -> SysResult<Fd> {
    let fd = block_on(kernel.socket(caller, AddressFamily::AF_UNIX, STREAM, 0))?;
    let len = sun(caller.memory(), name);
    block_on(kernel.bind(caller, fd, GuestPtr::new(0x900), len))?;
    block_on(kernel.listen(caller, fd, 4))?;
    Ok(fd)
}

fn unix_connect(kernel: &Kernel, caller: &Caller<'_>, name: &[u8]) -> SysResult<i32> {
    let fd = block_on(kernel.socket(caller, AddressFamily::AF_UNIX, STREAM, 0))?;
    let len = sun(caller.memory(), name);
    block_on(kernel.connect(caller, fd, GuestPtr::new(0x900), len))
}

#[test]
fn abstract_unix_names_are_per_network_namespace_and_paths_are_not() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    assert!(unix_listen(&kernel, &caller, b"\0ltp").is_ok());
    assert!(unix_listen(&kernel, &caller, b"/sock").is_ok());
    // Made before the unshare, a socket stays in the namespace it was made in.
    let early = block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, STREAM, 0)).unwrap();

    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWNET), Ok(0));

    assert_eq!(
        errno(unix_connect(&kernel, &caller, b"\0ltp")),
        Errno::ECONNREFUSED
    );
    assert_eq!(unix_connect(&kernel, &caller, b"/sock"), Ok(0));
    let len = sun(&memory, b"\0ltp");
    let connect = block_on(kernel.connect(&caller, early, GuestPtr::new(0x900), len));
    assert_eq!(connect, Ok(0));
    assert!(
        unix_listen(&kernel, &caller, b"\0ltp").is_ok(),
        "free in the new namespace"
    );
}

/// `SIOCGIF*` look at the interfaces of the namespace a socket was made in (`sock_ioctl`
/// passes `sock_net(sk)`), not at the caller's.
#[test]
fn interface_requests_look_at_the_sockets_network_namespace() {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        let loopback = wasixv2_vnet::Net::new(wasixv2_vnet::NetConfig::default(), None);
        kernel
            .with_network(loopback)
            .with_credentials(Credentials::new(0, 0))
    });
    let memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
    let caller = Caller::new(task, &memory);
    let socket = |family| block_on(kernel.socket(&caller, family, STREAM, 0)).unwrap();
    let index_of_lo = |fd: Fd| {
        put(&memory.0, 0x400, b"lo\0");
        ioctl(&kernel, &caller, fd, IoctlRequest::SIOCGIFINDEX)?;
        Ok::<_, SysErr>(u32::from_le_bytes(
            get(&memory.0, 0x410, 4).try_into().unwrap(),
        ))
    };
    let (inet, unix) = (
        socket(AddressFamily::AF_INET),
        socket(AddressFamily::AF_UNIX),
    );
    let lo = index_of_lo(unix).unwrap();

    // The new namespace has no IP network, so no interface at all.
    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWNET), Ok(0));

    assert_eq!(index_of_lo(inet), Ok(lo));
    assert_eq!(index_of_lo(unix), Ok(lo));
    let fresh = socket(AddressFamily::AF_UNIX);
    assert_eq!(errno(index_of_lo(fresh)), Errno::ENODEV);
}

/// `/proc/net/unix` lists every Unix socket of the reader's network namespace, bound or not,
/// with its type, state, inode and name (`unix_seq_show`): a connection nobody accepted yet
/// has no inode and shows as connecting, and a datagram socket another connected to is
/// established like its peer.
#[test]
fn proc_net_unix_lists_the_namespaces_sockets() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let ino = |fd: Fd| {
        let link = readlink(&kernel, &caller, &format!("/proc/self/fd/{}", fd.0)).unwrap();
        link.trim_start_matches("socket:[")
            .trim_end_matches(']')
            .to_string()
    };
    let abstract_listener = unix_listen(&kernel, &caller, b"\0ltp").unwrap();
    let path_listener = unix_listen(&kernel, &caller, b"/sock").unwrap();
    assert_eq!(unix_connect(&kernel, &caller, b"/sock"), Ok(0));
    let dgram =
        |kernel: &Kernel| block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, 2, 0)).unwrap();
    let (server, client) = (dgram(&kernel), dgram(&kernel));
    let len = sun(&memory, b"\0d\0x");
    assert_eq!(
        block_on(kernel.bind(&caller, server, GuestPtr::new(0x900), len)),
        Ok(0)
    );
    let connect = kernel.connect(&caller, client, GuestPtr::new(0x900), len);
    assert_eq!(block_on(connect), Ok(0));

    let text = read_text(&kernel, &caller, "/proc/net/unix");
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("Num       RefCount Protocol Flags    Type St Inode Path")
    );
    // Flags, type, state, inode and name of each; the client of the stream connection is the
    // socket after the listener, its unaccepted server end the one after that.
    let rows: Vec<Vec<String>> = lines
        .map(|line| {
            assert!(line.starts_with("00000000: "), "{line}");
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(fields[2], "00000000", "{line}");
            fields[3..].iter().map(|f| f.to_string()).collect()
        })
        .collect();
    let row = |fields: &[&str]| fields.iter().map(|f| f.to_string()).collect::<Vec<_>>();
    let (listening, none) = ("00010000", "00000000");
    let expected = [
        row(&[listening, "0001", "01", &ino(abstract_listener), "@ltp"]),
        row(&[listening, "0001", "01", &ino(path_listener), "/sock"]),
        row(&[none, "0001", "03", &ino(Fd(path_listener.0 + 1))]),
        row(&[none, "0001", "02", "0", "/sock"]),
        row(&[none, "0002", "03", &ino(server), "@d@x"]),
        row(&[none, "0002", "03", &ino(client)]),
    ];
    assert_eq!(rows, expected, "{text}");

    // Another namespace lists only its own; a closed socket is gone.
    assert_eq!(block_on(kernel.close(&caller, client)), Ok(0));
    let text = read_text(&kernel, &caller, "/proc/net/unix");
    assert_eq!(text.lines().count(), 6, "{text}");
    assert_eq!(unshare(&kernel, &caller, CloneFlags::CLONE_NEWNET), Ok(0));
    let fresh = dgram(&kernel);
    let text = read_text(&kernel, &caller, "/proc/net/unix");
    let only = format!(" 0002 01 {:>5}", ino(fresh));
    assert_eq!(text.lines().skip(1).collect::<Vec<_>>().len(), 1, "{text}");
    assert!(text.lines().nth(1).unwrap().ends_with(&only), "{text}");
}

/// `ifconfig lo:1 10.1.2.3 netmask 255.255.0.0`, `ifconfig lo:1 down`, `ifconfig lo mtu`, and
/// `ifconfig lo inet6 add` / `del` through the `SIOC*` requests ifconfig makes.
#[test]
fn ifconfig_requests_change_addresses() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let inet = block_on(kernel.socket(&caller, AddressFamily::AF_INET, DGRAM, 0)).unwrap();
    let inet6 = block_on(kernel.socket(&caller, AddressFamily::AF_INET6, DGRAM, 0)).unwrap();
    let unix = block_on(kernel.socket(&caller, AddressFamily::AF_UNIX, DGRAM, 0)).unwrap();
    let ifr = |name: &[u8], ip: [u8; 4]| {
        let mut raw = [0u8; 32];
        raw[..name.len()].copy_from_slice(name);
        raw[16..18].copy_from_slice(&(AddressFamily::AF_INET.0 as u16).to_le_bytes());
        raw[20..24].copy_from_slice(&ip);
        put(&memory.0, 0x400, &raw);
    };
    let sin_addr = || <[u8; 4]>::try_from(get(&memory.0, 0x414, 4)).unwrap();

    ifr(b"lo:1", [10, 1, 2, 3]);
    assert_eq!(
        ioctl(&kernel, &caller, inet, IoctlRequest::SIOCSIFADDR),
        Ok(0)
    );
    assert_eq!(
        errno(ioctl(&kernel, &caller, unix, IoctlRequest::SIOCSIFADDR)),
        Errno::ENOTTY
    );
    ifr(b"lo:1", [0; 4]);
    ioctl(&kernel, &caller, inet, IoctlRequest::SIOCGIFNETMASK).unwrap();
    assert_eq!(sin_addr(), [255, 0, 0, 0], "classful");
    ifr(b"lo:1", [255, 0, 255, 0]);
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet, IoctlRequest::SIOCSIFNETMASK)),
        Errno::EINVAL
    );
    ifr(b"lo:1", [255, 255, 0, 0]);
    assert_eq!(
        ioctl(&kernel, &caller, inet, IoctlRequest::SIOCSIFNETMASK),
        Ok(0)
    );
    ifr(b"lo:1", [0; 4]);
    ioctl(&kernel, &caller, inet, IoctlRequest::SIOCGIFADDR).unwrap();
    assert_eq!(sin_addr(), [10, 1, 2, 3]);
    ioctl(&kernel, &caller, inet, IoctlRequest::SIOCGIFNETMASK).unwrap();
    assert_eq!(sin_addr(), [255, 255, 0, 0]);
    ifr(b"lo", [0; 4]);
    ioctl(&kernel, &caller, inet, IoctlRequest::SIOCGIFADDR).unwrap();
    assert_eq!(sin_addr(), [127, 0, 0, 1], "lo's own label");
    // `struct ifconf`: no buffer asks for the length; a short one takes the whole entries
    // that fit.
    let ifconf = |len: i32, buf: u32| {
        put(
            &memory.0,
            0x400,
            &[len.to_le_bytes(), buf.to_le_bytes()].concat(),
        );
        ioctl(&kernel, &caller, unix, IoctlRequest::SIOCGIFCONF).unwrap();
        i32::from_le_bytes(get(&memory.0, 0x400, 4).try_into().unwrap())
    };
    assert_eq!(ifconf(0, 0), 64, "lo and lo:1");
    assert_eq!(ifconf(40, 0x800), 32);
    assert_eq!(get(&memory.0, 0x800, 3), b"lo\0");
    assert_eq!(get(&memory.0, 0x814, 4), [127, 0, 0, 1]);
    ifr(b"lo:1", [0; 4]);
    assert_eq!(
        ioctl(&kernel, &caller, inet, IoctlRequest::SIOCSIFFLAGS),
        Ok(0)
    );
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet, IoctlRequest::SIOCGIFADDR)),
        Errno::EADDRNOTAVAIL
    );

    put(&memory.0, 0x400, b"lo\0");
    put(&memory.0, 0x410, &9000i32.to_le_bytes());
    assert_eq!(
        ioctl(&kernel, &caller, unix, IoctlRequest::SIOCSIFMTU),
        Ok(0)
    );
    put(&memory.0, 0x410, &[0; 16]);
    ioctl(&kernel, &caller, inet, IoctlRequest::SIOCGIFMTU).unwrap();
    assert_eq!(get(&memory.0, 0x410, 4), 9000i32.to_le_bytes());
    ioctl(&kernel, &caller, inet, IoctlRequest::SIOCGIFHWADDR).unwrap();
    assert_eq!(
        get(&memory.0, 0x410, 2),
        772u16.to_le_bytes(),
        "ARPHRD_LOOPBACK"
    );

    // `struct in6_ifreq`: fd00::1/64 on interface 1.
    let mut req = vec![0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    req.extend(64u32.to_le_bytes());
    req.extend(1i32.to_le_bytes());
    put(&memory.0, 0x400, &req);
    assert_eq!(
        ioctl(&kernel, &caller, inet6, IoctlRequest::SIOCSIFADDR),
        Ok(0)
    );
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet6, IoctlRequest::SIOCSIFADDR)),
        Errno::EEXIST
    );
    assert_eq!(
        ioctl(&kernel, &caller, inet6, IoctlRequest::SIOCDIFADDR),
        Ok(0)
    );
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet6, IoctlRequest::SIOCDIFADDR)),
        Errno::EADDRNOTAVAIL
    );
}

/// `route add -net 10.9.0.0 netmask 255.255.0.0 dev lo` and `route del` (`SIOCADDRT`,
/// `SIOCDELRT` with a `struct rtentry`), and what they need.
#[test]
fn route_requests_change_the_routing_table() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let inet = block_on(kernel.socket(&caller, AddressFamily::AF_INET, DGRAM, 0)).unwrap();
    let sockaddr = |ip: [u8; 4]| {
        let mut raw = (AddressFamily::AF_INET.0 as u16).to_le_bytes().to_vec();
        raw.extend([0, 0]);
        raw.extend(ip);
        raw.extend([0; 8]);
        raw
    };
    let mut rt = vec![0u8; 4];
    rt.extend(sockaddr([10, 9, 0, 0]));
    rt.extend(sockaddr([0; 4]));
    rt.extend(sockaddr([255, 255, 0, 0]));
    rt.extend(1u16.to_le_bytes()); // RTF_UP
    rt.resize(68, 0);
    rt.extend(0x600u32.to_le_bytes());
    rt.resize(84, 0);
    put(&memory.0, 0x400, &rt);
    put(&memory.0, 0x600, b"lo\0");
    assert_eq!(
        ioctl(&kernel, &caller, inet, IoctlRequest::SIOCADDRT),
        Ok(0)
    );
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet, IoctlRequest::SIOCADDRT)),
        Errno::EEXIST
    );
    let route = read_text(&kernel, &caller, "/proc/net/route");
    assert!(route.contains("lo\t0000090A\t00000000\t0001"), "{route}");
    assert_eq!(
        ioctl(&kernel, &caller, inet, IoctlRequest::SIOCDELRT),
        Ok(0)
    );
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet, IoctlRequest::SIOCDELRT)),
        Errno::ESRCH
    );
    // A host part in the destination, and a family that is not AF_INET.
    put(&memory.0, 0x404, &sockaddr([10, 9, 0, 1]));
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet, IoctlRequest::SIOCADDRT)),
        Errno::EINVAL
    );
    put(&memory.0, 0x404, &[0; 2]);
    assert_eq!(
        errno(ioctl(&kernel, &caller, inet, IoctlRequest::SIOCADDRT)),
        Errno::EAFNOSUPPORT
    );
}

/// A sysfs mount shows the network namespace of whoever mounted it (`sysfs_get_tree`): the
/// boot one the initial namespace's, one made after `unshare(CLONE_NEWNET | CLONE_NEWNS)` the
/// new namespace's, as `ip netns exec` remounts `/sys` for.
#[test]
fn sysfs_shows_the_network_namespace_of_its_mount() {
    let (kernel, task, memory) = networked();
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/sys", kernel.sysfs(), MountFlags::default(), true)).unwrap();
    let caller = Caller::new(task, &memory);
    let read = |caller: &Caller<'_>, at: &str| read_text(&kernel, caller, at);
    assert_eq!(read(&caller, "/sys/class/net/lo/ifindex"), "1\n");
    assert_eq!(read(&caller, "/sys/class/net/lo/flags"), "0x9\n");
    assert_eq!(read(&caller, "/sys/class/net/lo/operstate"), "unknown\n");
    assert_eq!(
        readlink(&kernel, &caller, "/sys/class/net/lo").unwrap(),
        "../../devices/virtual/net/lo"
    );

    let child = fork(&kernel, &caller, CloneFlags(0));
    let child_caller = Caller::new(child, &memory);
    let flags = CloneFlags::CLONE_NEWNET | CloneFlags::CLONE_NEWNS;
    assert_eq!(unshare(&kernel, &child_caller, flags), Ok(0));
    assert_eq!(
        read(&child_caller, "/sys/class/net/lo/flags"),
        "0x9\n",
        "still the old mount"
    );
    let cstr = |addr: u32, s: &str| {
        put(&memory.0, addr, format!("{s}\0").as_bytes());
        GuestPtr::new(addr)
    };
    let (source, target, fstype) = (
        cstr(0x1100, "none"),
        cstr(0x1200, "/sys"),
        cstr(0x1300, "sysfs"),
    );
    let mount = kernel.mount(
        &child_caller,
        source,
        target,
        fstype,
        wasixv2_abi::MountFlags(0),
        GuestPtr::NULL,
    );
    assert_eq!(block_on(mount), Ok(0));
    assert_eq!(read(&child_caller, "/sys/class/net/lo/flags"), "0x8\n");
    assert_eq!(
        errno(
            open(&kernel, &child_caller, "/sys/class/net/lo/carrier").and_then(|fd| {
                block_on(kernel.read(&child_caller, fd, UserSliceMut::new(0x5000, 64)))
            })
        ),
        Errno::EINVAL,
        "carrier_show of a down interface"
    );
    assert_eq!(
        read(&caller, "/sys/class/net/lo/flags"),
        "0x9\n",
        "the parent's own"
    );
}

/// `ip netns add`: a bind mount of `/proc/self/ns/net` onto a file keeps the namespace, and
/// `ip netns exec` opens the file and enters it (`may_copy_tree` lets nsfs objects bind).
#[test]
fn a_namespace_file_binds_onto_a_file_and_setns_enters_it() {
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let child = fork(&kernel, &caller, CloneFlags(0));
    let child_caller = Caller::new(child, &memory);
    assert_eq!(
        unshare(&kernel, &child_caller, CloneFlags::CLONE_NEWNET),
        Ok(0)
    );
    let theirs = ns_ino(&kernel, &child_caller, "net");
    let name = path(&memory, "/netns-a");
    let flags = OpenFlags::O_CREAT | OpenFlags::O_RDONLY;
    let fd = block_on(kernel.openat(&caller, CWD, name, flags, 0o444)).unwrap();
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    let cstr = |addr: u32, s: &str| {
        put(&memory.0, addr, format!("{s}\0").as_bytes());
        GuestPtr::new(addr)
    };
    let source = cstr(0x1100, &format!("/proc/{}/ns/net", child.0));
    let target = cstr(0x1200, "/netns-a");
    let bind = wasixv2_abi::MountFlags::MS_BIND;
    let mount = kernel.mount(
        &caller,
        source,
        target,
        GuestPtr::NULL,
        bind,
        GuestPtr::NULL,
    );
    assert_eq!(block_on(mount), Ok(0));

    let file = open(&kernel, &caller, "/netns-a").unwrap();
    assert_ne!(ns_ino(&kernel, &caller, "net"), theirs);
    assert_eq!(
        setns(&kernel, &caller, file, CloneFlags::CLONE_NEWNET),
        Ok(0)
    );
    assert_eq!(ns_ino(&kernel, &caller, "net"), theirs);
}

/// An `AF_PACKET` socket, as `tst_ns_ifmove` opens one for `SIOCGIFINDEX`: `CAP_NET_RAW`, a
/// `sockaddr_ll` to bind and name it; no frame ever comes or goes.
#[test]
fn packet_sockets_answer_interface_requests_and_carry_no_frames() {
    use wasixv2_syscalls::{UserSlice, UserSliceMut};
    let (kernel, task, memory) = networked();
    let caller = Caller::new(task, &memory);
    let packet = |caller: &Caller<'_>, kind| {
        block_on(kernel.socket(caller, AddressFamily::AF_PACKET, kind, 0x0300))
    };
    assert_eq!(
        errno(packet(&caller, 1 /* SOCK_STREAM */)),
        Errno::ESOCKTNOSUPPORT
    );
    let fd = packet(&caller, 3 /* SOCK_RAW */ | 0x800 /* SOCK_NONBLOCK */).unwrap();
    put(&memory.0, 0x400, b"lo\0");
    assert_eq!(
        ioctl(&kernel, &caller, fd, IoctlRequest::SIOCGIFINDEX),
        Ok(0)
    );
    assert_eq!(get(&memory.0, 0x410, 4), 1i32.to_le_bytes());

    let sll = |index: i32| {
        let mut raw = (AddressFamily::AF_PACKET.0 as u16).to_le_bytes().to_vec();
        raw.extend([0, 0]);
        raw.extend(index.to_le_bytes());
        raw.resize(20, 0);
        put(&memory.0, 0x700, &raw);
        block_on(kernel.bind(&caller, fd, GuestPtr::new(0x700), 20))
    };
    assert_eq!(errno(sll(9)), Errno::ENODEV);
    assert_eq!(sll(1), Ok(0));
    put(&memory.0, 0x2f0, &20u32.to_le_bytes());
    let name = kernel.getsockname(
        &caller,
        fd,
        GuestPtrMut::new(0x300),
        GuestPtrMut::new(0x2f0),
    );
    assert_eq!(block_on(name), Ok(0));
    assert_eq!(
        get(&memory.0, 0x2f0, 4),
        18u32.to_le_bytes(),
        "sll_addr plus the MAC"
    );
    assert_eq!(
        get(&memory.0, 0x300, 10),
        [17, 0, 0, 3, 1, 0, 0, 0, 0x04, 0x03]
    );
    let recv = kernel.read(&caller, fd, UserSliceMut::new(0x500, 64));
    assert_eq!(errno(block_on(recv)), Errno::EAGAIN);
    let sent = kernel.sendto(
        &caller,
        fd,
        UserSlice::new(0x500, 14),
        wasixv2_abi::MsgFlags(0),
        GuestPtr::NULL,
        0,
    );
    assert_eq!(errno(block_on(sent)), Errno::EOPNOTSUPP);

    let user = fork(&kernel, &caller, CloneFlags(0));
    let user_caller = Caller::new(user, &memory);
    block_on(kernel.setuid(&user_caller, 1)).unwrap();
    assert_eq!(errno(packet(&user_caller, 3)), Errno::EPERM);
}
