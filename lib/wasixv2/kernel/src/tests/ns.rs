//! Namespaces: `/proc/<pid>/ns`, nsfs files and their `NS_GET_*` requests.

use wasixv2_abi::{CloneFlags, IoctlRequest, Statx, StatxMask};
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
    assert!(kernel.files(task).lock().unwrap().cloexec(owner).unwrap());
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
