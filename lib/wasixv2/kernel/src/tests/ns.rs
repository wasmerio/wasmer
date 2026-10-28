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
