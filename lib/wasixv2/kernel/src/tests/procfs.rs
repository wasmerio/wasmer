//! `/proc` and the character devices: magic links, the pseudo inodes behind pipes and event
//! descriptors, and device semantics.

use futures::FutureExt as _;
use wasixv2_abi::{
    ConstRlimit64Ptr, EventfdFlags, FcntlCmd, MapFlags, ProtFlags, ResolveFlags, Rlimit64,
    Rlimit64Ptr, RlimitResource, Statx,
};
use wasixv2_syscalls::{read_value, write_value};
use wasixv2_vfs::{FileType, MountFlags};

use super::ipc::{errno, get, pipe, put};
use super::*;
use crate::sync::MutexExt;

const CWD: Fd = Fd(-100);

/// A kernel with `/proc` and a `/dev` holding the device nodes, and one task.
fn machine() -> (Kernel, TaskId, Arc<Capture>) {
    let (kernel, task, stdout) = setup(b"");
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let dev = MemFs::new(Arc::new(HostClock(kernel.host.clone())), 0o755, 0, 0);
    block_on(vfs.mount(b"/dev", Arc::new(dev), MountFlags::default(), true)).unwrap();
    block_on(kernel.create_device_nodes(b"/dev")).unwrap();
    (kernel, task, stdout)
}

fn memory() -> FlatMemory {
    FlatMemory(RefCell::new(vec![0; 65536]))
}

/// Puts `path` NUL-terminated at 0x1000 and returns it.
fn path(memory: &FlatMemory, path: &str) -> Cstr {
    let mut bytes = path.as_bytes().to_vec();
    bytes.push(0);
    put(memory, 0x1000, &bytes);
    GuestPtr::new(0x1000)
}

/// `open_how` with `resolve` at 0x5000.
fn open_how(memory: &FlatMemory, resolve: u64) -> ConstOpenHowPtr {
    let bytes: Vec<u8> = [0u64, 0, resolve]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    put(memory, 0x5000, &bytes);
    GuestPtr::new(0x5000)
}

/// One task's calls, with its memory at hand.
struct T<'a> {
    kernel: &'a Kernel,
    caller: Caller<'a>,
    memory: &'a FlatMemory,
}

impl<'a> T<'a> {
    fn new(kernel: &'a Kernel, task: TaskId, memory: &'a FlatMemory) -> Self {
        Self {
            kernel,
            caller: Caller::new(task, memory),
            memory,
        }
    }

    fn open(&self, at: &str, flags: OpenFlags) -> SysResult<Fd> {
        let name = path(self.memory, at);
        block_on(self.kernel.openat(&self.caller, CWD, name, flags, 0o644))
    }

    fn readlink(&self, at: &str) -> SysResult<String> {
        let name = path(self.memory, at);
        let buf = GuestPtrMut::new(0x2000);
        let n = block_on(self.kernel.readlinkat(&self.caller, CWD, name, buf, 256))?;
        Ok(String::from_utf8(get(self.memory, 0x2000, n as usize)).unwrap())
    }

    fn link(&self, fd: Fd) -> SysResult<String> {
        self.readlink(&format!("/proc/self/fd/{}", fd.0))
    }

    fn stat(&self, at: &str, flags: AtFlags) -> SysResult<Statx> {
        let name = path(self.memory, at);
        self.statx(CWD, name, flags)
    }

    fn fstat(&self, fd: Fd) -> Statx {
        let name = path(self.memory, "");
        self.statx(fd, name, AtFlags::AT_EMPTY_PATH).unwrap()
    }

    fn statx(&self, dfd: Fd, name: Cstr, flags: AtFlags) -> SysResult<Statx> {
        let out = GuestPtrMut::new(0x3000);
        let mask = StatxMask(0x7ff);
        block_on(self.kernel.statx(&self.caller, dfd, name, flags, mask, out))?;
        Ok(block_on(read_value(self.memory, GuestPtr::new(0x3000))).unwrap())
    }

    fn read(&self, fd: Fd, len: u32) -> SysResult<u32> {
        block_on(
            self.kernel
                .read(&self.caller, fd, UserSliceMut::new(0x200, len)),
        )
    }

    fn write(&self, fd: Fd, data: &[u8]) -> SysResult<u32> {
        put(self.memory, 0x200, data);
        let buf = UserSlice::new(0x200, data.len() as u32);
        block_on(self.kernel.write(&self.caller, fd, buf))
    }

    fn read_all(&self, fd: Fd) -> String {
        let mut out = Vec::new();
        loop {
            let buf = UserSliceMut::new(0x4000, 4096);
            let n = block_on(self.kernel.read(&self.caller, fd, buf)).unwrap();
            if n == 0 {
                return String::from_utf8(out).unwrap();
            }
            out.extend(get(self.memory, 0x4000, n as usize));
        }
    }

    /// The names `getdents64` lists in the directory `at`.
    fn list(&self, at: &str) -> Vec<String> {
        let fd = self
            .open(at, OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY)
            .unwrap();
        let mut names = Vec::new();
        loop {
            let buf = GuestPtrMut::new(0x6000);
            let n = block_on(self.kernel.getdents64(&self.caller, fd, buf, 1024)).unwrap() as usize;
            if n == 0 {
                break;
            }
            let buf = get(self.memory, 0x6000, n);
            let mut at = 0;
            while at < n {
                let reclen = u16::from_le_bytes([buf[at + 16], buf[at + 17]]) as usize;
                let name = &buf[at + 19..at + reclen];
                let len = name.iter().position(|&b| b == 0).unwrap();
                names.push(String::from_utf8(name[..len].to_vec()).unwrap());
                at += reclen;
            }
        }
        block_on(self.kernel.close(&self.caller, fd)).unwrap();
        names
    }
}

#[test]
fn self_names_the_callers_process() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);

    assert_eq!(t.readlink("/proc/self"), Ok(task.0.to_string()));
    let thread = format!("{0}/task/{0}", task.0);
    assert_eq!(t.readlink("/proc/thread-self"), Ok(thread));
    let fd = t.open("/proc/self/comm", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read_all(fd), "\n");
    let fd = t.open("/proc/self/stat", OpenFlags::O_RDONLY).unwrap();
    let line = t.read_all(fd);
    assert!(line.starts_with(&format!("{} () S 0 ", task.0)), "{line}");
    assert_eq!(line.split(' ').count(), 52, "{line}");
}

/// `stat`'s `utime` and `stime` are the group's for the process and the thread's own for a
/// task, in ticks of `USER_HZ`; the machine's `cpu` line in `/proc/stat` sums every process.
#[test]
fn stat_shows_the_cpu_time_tasks_were_charged() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let caller = Caller::new(task, &memory);
    let spawn = kernel.thread_spawn(&caller, 0x10f00, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    let thread = block_on(spawn).unwrap();
    while kernel.take_pending(task).is_some() {}
    let ms = std::time::Duration::from_millis;
    let account = |task| wasixv2_syscalls::CpuTime::cpu_account(&kernel, task);
    account(task).charge(ms(120), ms(30));
    account(TaskId(thread as u32)).charge(ms(50), ms(0));
    let times = |path: &str| {
        let fd = t.open(path, OpenFlags::O_RDONLY).unwrap();
        let line = t.read_all(fd);
        let fields: Vec<&str> = line.rsplit(") ").next().unwrap().split(' ').collect();
        // utime, stime, cutime, cstime: fields 14 to 17, 12 to 15 after the comm.
        fields[11..15].join(" ")
    };
    assert_eq!(times("/proc/self/stat"), "17 3 0 0");
    assert_eq!(times(&format!("/proc/self/task/{thread}/stat")), "5 0 0 0");
    let fd = t.open("/proc/stat", OpenFlags::O_RDONLY).unwrap();
    let stat = t.read_all(fd);
    assert!(stat.starts_with("cpu  17 0 3 "), "{stat}");
}

/// A task that took its part in a group stop is `T (stopped)`; one yet to take it is not.
#[test]
fn a_stopped_task_shows_as_stopped() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let caller = Caller::new(task, &memory);
    let spawn = kernel.thread_spawn(&caller, 0x10f00, 0, GuestPtrMut::NULL, 0, GuestPtrMut::NULL);
    let thread = block_on(spawn).unwrap();
    while kernel.take_pending(task).is_some() {}

    let stop = kernel.tgkill(&caller, task.0 as Pid, thread, Signal::SIGSTOP);
    assert_eq!(block_on(stop), Ok(0));
    assert_eq!(
        kernel.take_pending(TaskId(thread as u32)),
        Some(GuestAction::Stopped)
    );

    let stat = format!("/proc/{0}/task/{thread}/stat", task.0);
    let fd = t.open(&stat, OpenFlags::O_RDONLY).unwrap();
    let line = t.read_all(fd);
    assert!(line.starts_with(&format!("{thread} () T ")), "{line}");
    let status = format!("/proc/{0}/task/{thread}/status", task.0);
    let fd = t.open(&status, OpenFlags::O_RDONLY).unwrap();
    assert!(t.read_all(fd).contains("State:\tT (stopped)\n"));
    let fd = t.open("/proc/self/status", OpenFlags::O_RDONLY).unwrap();
    assert!(t.read_all(fd).contains("State:\tS (sleeping)\n"));
}

/// `task_seccomp`: status shows the task's `no_new_privs`, seccomp mode and filter count.
#[test]
fn status_shows_no_new_privs_and_seccomp() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let caller = Caller::new(task, &memory);
    let fd = t.open("/proc/self/status", OpenFlags::O_RDONLY).unwrap();
    assert!(
        t.read_all(fd)
            .contains("NoNewPrivs:\t0\nSeccomp:\t0\nSeccomp_filters:\t0\n")
    );
    let prctl = |option, arg2| block_on(kernel.prctl(&caller, option, arg2, 0, 0, 0));
    assert_eq!(prctl(PrctlOption::PR_SET_NO_NEW_PRIVS, 1), Ok(0));
    assert_eq!(prctl(PrctlOption::PR_SET_SECCOMP, 1), Ok(0));
    let fd = t.open("/proc/self/status", OpenFlags::O_RDONLY).unwrap();
    assert!(
        t.read_all(fd)
            .contains("NoNewPrivs:\t1\nSeccomp:\t1\nSeccomp_filters:\t0\n")
    );
}

#[test]
fn fd_links_name_files_and_pseudo_inodes() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let file = t
        .open("/f", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    let (r, _w) = pipe(&kernel, &t.caller, OpenFlags(0));
    let event = block_on(kernel.eventfd2(&t.caller, 0, EventfdFlags(0))).unwrap();

    assert_eq!(t.link(file), Ok("/f".into()));
    let ino = t.fstat(r).stx_ino;
    assert_eq!(t.link(r), Ok(format!("pipe:[{ino}]")));
    assert_eq!(t.link(event), Ok("anon_inode:[eventfd]".into()));
    // Anonymous inodes have no file type.
    assert_eq!(t.fstat(event).stx_mode, 0o600);
    let fifo = FileType::Fifo.mode_bits() | 0o600;
    assert_eq!(u32::from(t.fstat(r).stx_mode), fifo);
    let unlinked = block_on(kernel.unlinkat(&t.caller, CWD, path(&memory, "/f"), AtFlags(0)));
    assert_eq!(unlinked, Ok(0));
    assert_eq!(t.link(file), Ok("/f (deleted)".into()));
    assert_eq!(errno(t.link(Fd(77))), Errno::ENOENT);
}

#[test]
fn opening_a_pipes_fd_link_attaches_to_the_same_pipe() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let (r, w) = pipe(&kernel, &t.caller, OpenFlags::O_NONBLOCK);
    let again = t
        .open(&format!("/proc/self/fd/{}", w.0), OpenFlags::O_WRONLY)
        .unwrap();

    assert_eq!(t.write(again, b"via proc"), Ok(8));
    assert_eq!(block_on(kernel.close(&t.caller, w)), Ok(0));
    assert_eq!(block_on(kernel.close(&t.caller, again)), Ok(0));
    assert_eq!(t.read_all(r), "via proc");
}

#[test]
fn a_pipes_fd_link_opens_without_waiting_for_a_partner() {
    // `fifo_open` waits for a partner only on a FIFO, not on a pipe's own inode: `cat <(...)`
    // opens `/dev/fd/N` after the writer may have finished.
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let (r, w) = pipe(&kernel, &t.caller, OpenFlags(0));
    assert_eq!(t.write(w, b"left"), Ok(4));
    assert_eq!(block_on(kernel.close(&t.caller, w)), Ok(0));
    let at = path(&memory, &format!("/proc/self/fd/{}", r.0));
    let opened = kernel
        .openat(&t.caller, CWD, at, OpenFlags::O_RDONLY, 0)
        .now_or_never();
    let again = opened.expect("the open waits for a writer").unwrap();
    assert_eq!(t.read_all(again), "left");

    // Nor does a writer wait for a reader, or get `ENXIO` without one: it gets `EPIPE`.
    let (r, w) = pipe(&kernel, &t.caller, OpenFlags(0));
    assert_eq!(block_on(kernel.close(&t.caller, r)), Ok(0));
    for flags in [
        OpenFlags::O_WRONLY,
        OpenFlags::O_WRONLY | OpenFlags::O_NONBLOCK,
    ] {
        let at = path(&memory, &format!("/proc/self/fd/{}", w.0));
        let opened = kernel.openat(&t.caller, CWD, at, flags, 0).now_or_never();
        let again = opened.expect("the open waits for a reader").unwrap();
        assert_eq!(errno(t.write(again, b"x")), Errno::EPIPE);
    }
}

#[test]
fn opening_stdouts_fd_link_reaches_the_same_stream() {
    let (kernel, task, stdout) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let fd = t.open("/proc/self/fd/1", OpenFlags::O_WRONLY).unwrap();

    assert_eq!(t.write(fd, b"out"), Ok(3));
    assert_eq!(*stdout.0.lock_or_recover(), b"out");
    // Anonymous files do not open again: the shared inode is root's, `0600`, so `may_open`
    // refuses a user before `no_open` would.
    let event = block_on(kernel.eventfd2(&t.caller, 0, EventfdFlags(0))).unwrap();
    let at = format!("/proc/self/fd/{}", event.0);
    assert_eq!(errno(t.open(&at, OpenFlags::O_RDWR)), Errno::EACCES);
}

#[test]
fn magic_links_jump_to_the_object_of_an_o_path_descriptor() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let fd = t
        .open("/f", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    let opath = t.open("/f", OpenFlags::O_PATH).unwrap();
    let at = format!("/proc/self/fd/{}", opath.0);

    // musl's fchmod on an O_PATH descriptor.
    let name = path(&memory, &at);
    assert_eq!(
        block_on(kernel.fchmodat(&t.caller, CWD, name, 0o600)),
        Ok(0)
    );
    assert_eq!(t.fstat(fd).stx_mode & 0o7777, 0o600);
    // Following the link stats the file; not following it stats the link.
    let followed = t.stat(&at, AtFlags(0)).unwrap();
    assert_eq!(followed.stx_ino, t.fstat(fd).stx_ino);
    let link = t.stat(&at, AtFlags::AT_SYMLINK_NOFOLLOW).unwrap();
    assert_eq!(
        u32::from(link.stx_mode) & 0o170000,
        FileType::Symlink.mode_bits()
    );
    // A jump in the middle of a path continues from the directory.
    let cwd = t.stat("/proc/self/cwd/f", AtFlags(0)).unwrap();
    assert_eq!(cwd.stx_ino, followed.stx_ino);
    // O_CREAT on a link to an existing file opens it.
    let created = t.open(&at, OpenFlags::O_CREAT | OpenFlags::O_RDWR);
    assert!(created.is_ok(), "{created:?}");
}

#[test]
fn resolve_flags_refuse_magic_links() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let proc = t.open("/proc", OpenFlags::O_PATH).unwrap();

    let name = path(&memory, "/proc/self/fd/0");
    let how = open_how(&memory, ResolveFlags::RESOLVE_NO_MAGICLINKS.0);
    let refused = block_on(kernel.openat2(&t.caller, CWD, name, how, 24));
    assert_eq!(errno(refused), Errno::ELOOP);
    // A scoped resolution cannot jump at all.
    let name = path(&memory, "self/fd/0");
    let how = open_how(&memory, ResolveFlags::RESOLVE_BENEATH.0);
    let beneath = block_on(kernel.openat2(&t.caller, proc, name, how, 24));
    assert_eq!(errno(beneath), Errno::EXDEV);
}

#[test]
fn proc_lists_processes_and_their_descriptors() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);

    let root = t.list("/proc");
    assert!(root.contains(&"self".to_string()), "{root:?}");
    assert!(root.contains(&task.0.to_string()), "{root:?}");
    // The listing's own descriptor is open too.
    assert_eq!(t.list("/proc/self/fd"), [".", "..", "0", "1", "2", "3"]);
    assert_eq!(t.list("/proc/self/task"), [".", "..", &task.0.to_string()]);
}

#[test]
fn vm_tunables_show_the_memory_model() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);

    let fd = t
        .open("/proc/sys/vm/mmap_min_addr", OpenFlags::O_RDONLY)
        .unwrap();
    assert_eq!(t.read_all(fd), "65536\n");
    let vm = t.list("/proc/sys/vm");
    assert!(vm.contains(&"mmap_min_addr".to_string()), "{vm:?}");
}

/// The `fs.*` tunables the kernel acts on read as integers and take what root writes:
/// `dir-notify-enable` and `leases-enable` (1), `lease-break-time` (45 seconds).
#[test]
fn fs_tunables_read_and_take_integers() {
    let (kernel, task, _) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(0, 0)));
    let procfs = kernel.procfs();
    let mount = kernel
        .vfs()
        .mount(b"/proc", procfs, MountFlags::default(), true);
    block_on(mount).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let tunables = [
        ("dir-notify-enable", "1\n"),
        ("leases-enable", "1\n"),
        ("lease-break-time", "45\n"),
    ];
    let fs = t.list("/proc/sys/fs");
    for (name, value) in tunables {
        assert!(fs.contains(&name.to_string()), "{fs:?}");
        let at = format!("/proc/sys/fs/{name}");
        let fd = t.open(&at, OpenFlags::O_RDONLY).unwrap();
        assert_eq!(t.read_all(fd), value);
        let fd = t.open(&at, OpenFlags::O_WRONLY).unwrap();
        assert_eq!(t.write(fd, b" -3\n"), Ok(4));
        assert_eq!(errno(t.write(fd, b"x")), Errno::EINVAL);
        let fd = t.open(&at, OpenFlags::O_RDONLY).unwrap();
        assert_eq!(t.read_all(fd), "-3\n");
    }
    assert_eq!(kernel.sysctls.lease_break_time(), -3);
}

#[test]
fn memory_devices_read_write_and_seek_like_linux() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let rw = OpenFlags::O_RDWR;
    let null = t.open("/dev/null", rw).unwrap();
    let zero = t.open("/dev/zero", rw).unwrap();
    let full = t.open("/dev/full", rw).unwrap();
    let urandom = t.open("/dev/urandom", OpenFlags::O_RDONLY).unwrap();

    assert_eq!(t.read(null, 16), Ok(0));
    assert_eq!(t.write(null, &[7; 16]), Ok(16));
    assert_eq!(t.read(zero, 16), Ok(16));
    assert_eq!(get(&memory, 0x200, 16), [0; 16]);
    assert_eq!(errno(t.write(full, &[7; 16])), Errno::ENOSPC);
    assert_eq!(t.read(urandom, 16), Ok(16));
    assert_eq!(get(&memory, 0x200, 16), [0x5a; 16]);
    let pread = block_on(kernel.pread64(&t.caller, zero, UserSliceMut::new(0x200, 4), 100));
    assert_eq!(pread, Ok(4));
    let lseek = |fd| block_on(kernel.lseek(&t.caller, fd, 123, SeekWhence::SEEK_SET));
    assert_eq!(lseek(null), Ok(0));
    assert_eq!(lseek(urandom), Ok(0));
    let attr = t.fstat(null);
    assert_eq!((attr.stx_rdev_major, attr.stx_rdev_minor), (1, 3));
    assert_eq!(t.link(null), Ok("/dev/null".into()));
    // No process has a controlling terminal.
    assert_eq!(errno(t.open("/dev/tty", rw)), Errno::ENXIO);
}

#[test]
fn memory_and_random_devices_fill_reads_larger_than_one_step() {
    // `dd bs=1M` counts on it: `read_iter_zero` and `urandom_read_iter` fill the whole buffer.
    let (kernel, task, _) = machine();
    let memory = FlatMemory(RefCell::new(vec![0; 3 << 20]));
    let t = T::new(&kernel, task, &memory);
    let len = 1 << 20;
    for (at, fill) in [("/dev/zero", 0), ("/dev/urandom", 0x5a)] {
        let fd = t.open(at, OpenFlags::O_RDONLY).unwrap();
        put(&memory, 0x10000, &vec![7; len as usize]);
        let read = block_on(kernel.read(&t.caller, fd, UserSliceMut::new(0x10000, len)));
        assert_eq!(read, Ok(len), "{at}");
        assert!(
            get(&memory, 0x10000, len as usize)
                .iter()
                .all(|&b| b == fill),
            "{at}"
        );
        let pread = kernel.pread64(&t.caller, fd, UserSliceMut::new(0x10000, len), 0);
        assert_eq!(block_on(pread), Ok(len), "{at}");
    }
}

/// A `/proc/<pid>` directory stands for its process in `pidfd_send_signal`, as a pidfd does
/// (`pidfd_to_pid`); no other call takes it, nor a thread's directory, another proc file or a
/// location-only descriptor.
#[test]
fn pidfd_send_signal_takes_a_proc_pid_directory() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let dir = format!("/proc/{}", task.0);
    let send = |fd: Fd, sig: Signal| {
        block_on(kernel.pidfd_send_signal(&t.caller, fd, sig, GuestPtr::NULL, 0))
    };
    let open = |at: &str, flags: OpenFlags| t.open(at, flags | OpenFlags::O_RDONLY).unwrap();

    let pid_dir = open(&dir, OpenFlags::O_DIRECTORY);
    assert_eq!(send(pid_dir, Signal(0)), Ok(0));
    assert_eq!(send(pid_dir, Signal::SIGSTOP), Ok(0));
    assert_eq!(kernel.take_pending(task), Some(GuestAction::Stopped));

    let thread_dir = open(&format!("{dir}/task/{}", task.0), OpenFlags::O_DIRECTORY);
    let stat = open(&format!("{dir}/stat"), OpenFlags::empty());
    let location = open(&dir, OpenFlags::O_PATH);
    for fd in [thread_dir, stat, location] {
        assert_eq!(send(fd, Signal(0)), Err(Errno::EBADF.into()));
    }
    let waitid = kernel.waitid(
        &t.caller,
        wasixv2_abi::Idtype::P_PIDFD,
        pid_dir.0,
        GuestPtrMut::NULL,
        wasixv2_abi::WaitOptions::WEXITED,
        GuestPtrMut::NULL,
    );
    assert_eq!(block_on(waitid), Err(Errno::EBADF.into()), "pidfd_get_pid");
}

/// `fs.nr_open` takes what `proc_douintvec_minmax` takes, from root alone, and bounds every
/// descriptor table as `fs/file.c` does: no table grows to it (`expand_files`, which `dup3`
/// reports as `EBADF`), no copy of a larger one is made (`dup_fd`), and no hard
/// `RLIMIT_NOFILE` exceeds it.
#[test]
fn nr_open_bounds_descriptor_tables_and_rlimit_nofile() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let nr_open = "/proc/sys/fs/nr_open";
    let set = |value: &str| {
        let fd = t.open(nr_open, OpenFlags::O_WRONLY).unwrap();
        let written = t.write(fd, value.as_bytes());
        block_on(kernel.close(&t.caller, fd)).unwrap();
        written
    };
    let get = || {
        let fd = t.open(nr_open, OpenFlags::O_RDONLY).unwrap();
        t.read_all(fd)
    };
    let dup3 = |new: i32| block_on(kernel.dup3(&t.caller, Fd(0), Fd(new), OpenFlags::empty()));

    assert_eq!(get(), "1048576\n");
    // Below `BITS_PER_LONG`, or above `sysctl_nr_open_max` of a 32-bit kernel.
    assert_eq!(set("31\n"), Err(Errno::ERANGE.into()));
    assert_eq!(set("1073741793"), Err(Errno::ERANGE.into()));
    assert_eq!(set("1073741792"), Ok(10));
    assert_eq!(dup3(65), Ok(Fd(65)));

    // A copy of the table would need 96 descriptors.
    assert_eq!(set("64\n"), Ok(3));
    assert_eq!(get(), "64\n");
    let forkable = super::proc::forkable();
    let forker = Caller::new(task, &forkable);
    let sigchld = Signal::SIGCHLD.0 as u32;
    let null = GuestPtrMut::NULL;
    let fork = |flags: u32| block_on(kernel.clone(&forker, flags | sigchld, 0, null, 0, null));
    assert_eq!(fork(0), Err(Errno::EMFILE.into()));
    assert_eq!(kernel.take_pending(task), None);
    block_on(kernel.close(&t.caller, Fd(65))).unwrap();
    assert!(
        fork(0).is_ok(),
        "the table copies once its highest descriptor went"
    );
    let Some(GuestAction::Fork { child, .. }) = kernel.take_pending(task) else {
        panic!("fork left no child");
    };

    // The parent's table grew to 256 descriptors before and keeps them; the child's copy
    // holds the default 32 and grows to 64 at most.
    assert_eq!(dup3(255), Ok(Fd(255)));
    let child_memory = super::procfs::memory();
    let child = Caller::new(child, &child_memory);
    let dup3 = |new: i32| block_on(kernel.dup3(&child, Fd(0), Fd(new), OpenFlags::empty()));
    assert_eq!(dup3(63), Ok(Fd(63)));
    assert_eq!(dup3(64), Err(Errno::EBADF.into()));
    let dupfd = kernel.fcntl64(&child, Fd(0), FcntlCmd::F_DUPFD, 64);
    assert_eq!(block_on(dupfd), Err(Errno::EMFILE.into()));

    let limit = |max: u64| {
        let limit = Rlimit64 {
            rlim_cur: 32,
            rlim_max: max,
        };
        block_on(write_value(&memory, Rlimit64Ptr::new(0x400), &limit)).unwrap();
        let new = ConstRlimit64Ptr::new(0x400);
        let nofile = RlimitResource::RLIMIT_NOFILE;
        block_on(kernel.prlimit64(&t.caller, 0, nofile, new, Rlimit64Ptr::NULL))
    };
    assert_eq!(limit(65), Err(Errno::EPERM.into()));
    assert_eq!(limit(64), Ok(0));

    // Only root writes it.
    block_on(kernel.setresuid(&t.caller, 1, 1, 1)).unwrap();
    assert_eq!(
        t.open(nr_open, OpenFlags::O_WRONLY),
        Err(Errno::EACCES.into())
    );
}

/// `kernel.tainted` reads 0, as nothing taints this kernel, and takes flags from a writer
/// with `CAP_SYS_ADMIN` alone (`proc_taint`): each known flag written is added, none is
/// cleared, and bits past `TAINT_FLAGS_COUNT` are ignored. Root without the capability
/// opens it, as its mode lets root, and fails to write (`EPERM`); anyone else cannot open it
/// to write.
#[test]
fn kernel_tainted_takes_flags_from_cap_sys_admin() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let tainted = "/proc/sys/kernel/tainted";
    let set = |value: &str| {
        let fd = t.open(tainted, OpenFlags::O_WRONLY)?;
        let written = t.write(fd, value.as_bytes());
        block_on(kernel.close(&t.caller, fd)).unwrap();
        written
    };
    let get = || {
        let fd = t.open(tainted, OpenFlags::O_RDONLY).unwrap();
        t.read_all(fd)
    };

    assert_eq!(get(), "0\n");
    let mode = t.stat(tainted, AtFlags::empty()).unwrap().stx_mode;
    assert_eq!(mode & 0o7777, 0o644);
    assert_eq!(set("4\n"), Ok(2));
    assert_eq!(set("1"), Ok(1));
    assert_eq!(get(), "5\n");
    assert_eq!(set("0"), Ok(1), "nothing is cleared");
    assert_eq!(set(&(1u32 << 21 | 1 << 9).to_string()), Ok(7));
    assert_eq!(get(), "517\n", "only TAINT_FLAGS_COUNT flags are kept");
    assert_eq!(set("-1"), Err(Errno::EINVAL.into()));
    assert_eq!(set("W"), Err(Errno::EINVAL.into()));

    let all = u64::MAX >> (63 - u64::from(wasixv2_vfs::Cap::LAST));
    let sys_admin = wasixv2_vfs::Cap::SysAdmin.bit();
    super::cred::set_caps(&kernel, &t.caller, &memory, all & !sys_admin);
    assert_eq!(set("2"), Err(Errno::EPERM.into()));
    assert_eq!(get(), "517\n");

    block_on(kernel.setresuid(&t.caller, 1, 1, 1)).unwrap();
    assert_eq!(
        t.open(tainted, OpenFlags::O_WRONLY),
        Err(Errno::EACCES.into())
    );
    assert_eq!(get(), "517\n");
}

/// A sysctl's value as root writes it, through `t`.
fn sysctl_set(t: &T<'_>, at: &str, value: &str) -> SysResult<u32> {
    let fd = t.open(at, OpenFlags::O_WRONLY)?;
    let written = t.write(fd, value.as_bytes());
    block_on(t.kernel.close(&t.caller, fd)).unwrap();
    written
}

fn sysctl_get(t: &T<'_>, at: &str) -> String {
    let fd = t.open(at, OpenFlags::O_RDONLY).unwrap();
    t.read_all(fd)
}

/// `fs.aio-nr` counts what the AIO contexts asked for and `fs.aio-max-nr` bounds it
/// (`fs/aio.c`'s sysctls; the AIO tests count contexts).
#[test]
fn aio_sysctls_show_the_count_and_take_a_new_limit() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let max = "/proc/sys/fs/aio-max-nr";
    assert_eq!(sysctl_get(&t, "/proc/sys/fs/aio-nr"), "0\n");
    assert_eq!(sysctl_get(&t, max), "65536\n");
    assert_eq!(sysctl_set(&t, max, "100"), Ok(3));
    assert_eq!(sysctl_get(&t, max), "100\n");
    assert_eq!(kernel.aio.max(), 100);
}

/// `fs.pipe-max-size` takes a size and keeps it rounded as a pipe's (`proc_dopipe_max_size`,
/// `round_pipe_size`), and bounds callers without `CAP_SYS_RESOURCE` as `fs/pipe.c` does: a
/// new pipe holds at most that much (`alloc_pipe_info`, which LTP fcntl35 checks), and
/// `F_SETPIPE_SZ` grows none past it (`EPERM`) but always shrinks. With the capability,
/// neither applies.
#[test]
fn pipe_max_size_bounds_unprivileged_pipes() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let max = "/proc/sys/fs/pipe-max-size";
    let size = |fd: Fd, cmd: FcntlCmd, arg: u32| block_on(kernel.fcntl64(&t.caller, fd, cmd, arg));

    assert_eq!(sysctl_get(&t, max), "1048576\n");
    assert_eq!(sysctl_set(&t, max, "4097"), Ok(4));
    assert_eq!(sysctl_get(&t, max), "8192\n");
    assert_eq!(sysctl_set(&t, max, "2147483649"), Err(Errno::ERANGE.into()));
    assert_eq!(sysctl_set(&t, max, "-1"), Err(Errno::EINVAL.into()));
    assert_eq!(sysctl_get(&t, max), "8192\n");

    let (_, privileged) = pipe(&kernel, &t.caller, OpenFlags::empty());
    assert_eq!(size(privileged, FcntlCmd::F_GETPIPE_SZ, 0), Ok(65536));
    assert_eq!(
        size(privileged, FcntlCmd::F_SETPIPE_SZ, 1 << 21),
        Ok(1 << 21)
    );

    let all = u64::MAX >> (63 - u64::from(wasixv2_vfs::Cap::LAST));
    let resource = wasixv2_vfs::Cap::SysResource.bit();
    super::cred::set_caps(&kernel, &t.caller, &memory, all & !resource);
    let (_, fd) = pipe(&kernel, &t.caller, OpenFlags::empty());
    assert_eq!(size(fd, FcntlCmd::F_GETPIPE_SZ, 0), Ok(8192));
    assert_eq!(
        size(fd, FcntlCmd::F_SETPIPE_SZ, 8193),
        Err(Errno::EPERM.into())
    );
    assert_eq!(size(fd, FcntlCmd::F_SETPIPE_SZ, 0), Ok(4096));
    assert_eq!(size(fd, FcntlCmd::F_SETPIPE_SZ, 8192), Ok(8192));
    // A pipe already past the limit may stay there, and shrink.
    assert_eq!(
        size(privileged, FcntlCmd::F_SETPIPE_SZ, 1 << 21),
        Ok(1 << 21)
    );
    assert_eq!(
        size(privileged, FcntlCmd::F_SETPIPE_SZ, 1 << 20),
        Ok(1 << 20)
    );
}

/// The `vm.overcommit_*` values are kept as `mm/util.c` takes them, and `CommitLimit` follows
/// them as `vm_commit_limit` does: the ratio of memory, or the size once one is set, which
/// a later ratio clears again.
#[test]
fn overcommit_sysctls_are_kept_and_set_the_commit_limit() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let vm = |name: &str| format!("/proc/sys/vm/{name}");
    let commit_limit = || {
        let meminfo = sysctl_get(&t, "/proc/meminfo");
        let line = meminfo.lines().find(|l| l.starts_with("CommitLimit:"));
        line.unwrap().split_whitespace().nth(1).unwrap().to_string()
    };

    assert_eq!(sysctl_get(&t, &vm("overcommit_memory")), "0\n");
    assert_eq!(sysctl_get(&t, &vm("overcommit_ratio")), "50\n");
    assert_eq!(sysctl_get(&t, &vm("overcommit_kbytes")), "0\n");
    assert_eq!(commit_limit(), "2097152");

    assert_eq!(
        sysctl_set(&t, &vm("overcommit_memory"), "3"),
        Err(Errno::EINVAL.into())
    );
    assert_eq!(
        sysctl_set(&t, &vm("overcommit_memory"), "-1"),
        Err(Errno::EINVAL.into())
    );
    assert_eq!(sysctl_set(&t, &vm("overcommit_memory"), "2\n"), Ok(2));
    assert_eq!(sysctl_get(&t, &vm("overcommit_memory")), "2\n");

    assert_eq!(sysctl_set(&t, &vm("overcommit_ratio"), "25"), Ok(2));
    assert_eq!(commit_limit(), "1048576");
    assert_eq!(sysctl_set(&t, &vm("overcommit_kbytes"), "1001"), Ok(4));
    assert_eq!(sysctl_get(&t, &vm("overcommit_ratio")), "0\n");
    assert_eq!(commit_limit(), "1000");
    assert_eq!(sysctl_set(&t, &vm("overcommit_ratio"), "10"), Ok(2));
    assert_eq!(sysctl_get(&t, &vm("overcommit_kbytes")), "0\n");
    assert_eq!(commit_limit(), "419428");

    block_on(kernel.setresuid(&t.caller, 1, 1, 1)).unwrap();
    let write = OpenFlags::O_WRONLY;
    assert_eq!(
        t.open(&vm("overcommit_memory"), write),
        Err(Errno::EACCES.into())
    );
}

/// `vm.max_map_count` bounds the areas of an address space as `mm/mmap.c` and `mm/vma.c` do:
/// `mmap` and the break fail with `ENOMEM` once the count is past it, so it ends one past it
/// (as LTP max_map_count expects, counting `/proc/<pid>/maps`), and a `munmap` or `mprotect`
/// that splits an area fails where a split would leave the count at it (`split_vma`).
#[test]
fn max_map_count_bounds_the_areas_of_an_address_space() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let limit = "/proc/sys/vm/max_map_count";
    let page = wasixv2_syscalls::WASM_PAGE_SIZE as u32;
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let (read, write) = (
        ProtFlags::PROT_READ,
        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
    );
    let mmap =
        |len: u32, prot: ProtFlags| block_on(kernel.mmap(&t.caller, 0, len, prot, anon, Fd(-1), 0));
    let maps = || sysctl_get(&t, "/proc/self/maps");
    let count = || maps().lines().count();

    assert_eq!(sysctl_get(&t, limit), "65530\n");
    assert_eq!(sysctl_set(&t, limit, "-1"), Err(Errno::EINVAL.into()));

    // Neighbours with other protections are areas of their own.
    assert_eq!(sysctl_set(&t, limit, "4"), Ok(1));
    let mut made = 0;
    while mmap(page, if made % 2 == 0 { read } else { write }).is_ok() {
        made += 1;
    }
    assert_eq!(count(), 5, "{}", maps());
    assert_eq!(mmap(page, read), Err(Errno::ENOMEM.into()));
    assert!(maps().contains(" r--p "), "{}", maps());

    // A hole in the middle of an area splits it in two.
    assert_eq!(sysctl_set(&t, limit, "100"), Ok(3));
    let three = mmap(3 * page, read).unwrap();
    assert_eq!(count(), 6, "{}", maps());
    assert_eq!(sysctl_set(&t, limit, "6"), Ok(1));
    let munmap = |at: u32| block_on(kernel.munmap(&t.caller, at, page));
    assert_eq!(munmap(three + page), Err(Errno::ENOMEM.into()));
    assert_eq!(sysctl_set(&t, limit, "7"), Ok(1));
    assert_eq!(munmap(three + page), Ok(0));
    assert_eq!(count(), 7, "{}", maps());

    // Changing the protection inside an area splits it twice; the same protection splits
    // nothing.
    assert_eq!(sysctl_set(&t, limit, "100"), Ok(3));
    let other = mmap(3 * page, write).unwrap();
    assert_eq!(count(), 8, "{}", maps());
    assert_eq!(sysctl_set(&t, limit, "9"), Ok(1));
    let mprotect = |prot: ProtFlags| block_on(kernel.mprotect(&t.caller, other + page, page, prot));
    assert_eq!(mprotect(read), Err(Errno::ENOMEM.into()));
    assert_eq!(mprotect(write), Ok(0));
    assert_eq!(sysctl_set(&t, limit, "10"), Ok(2));
    assert_eq!(mprotect(read), Ok(0));
    assert_eq!(count(), 10, "{}", maps());

    // The break makes no new area past the limit either.
    assert_eq!(sysctl_set(&t, limit, "9"), Ok(1));
    let brk = |at: u32| block_on(kernel.brk(&t.caller, at)).unwrap() as u32;
    let start = brk(0);
    assert_eq!(brk(start + page), start);
    assert_eq!(sysctl_set(&t, limit, "10"), Ok(2));
    assert_eq!(brk(start + page), start + page);
    assert_eq!(count(), 11, "{}", maps());
}

/// Opening a proc file with `O_TRUNC`, as `fopen(path, "w")` and a shell's `>` do, succeeds
/// and truncates nothing (`proc_setattr`, `proc_sys_setattr`); its mode and owner stay the
/// kernel's (`EPERM`).
#[test]
fn a_proc_file_opens_with_o_trunc_and_keeps_its_mode() {
    let (kernel, task, _) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let truncating = OpenFlags::O_WRONLY | OpenFlags::O_CREAT | OpenFlags::O_TRUNC;

    let fd = t.open("/proc/self/comm", truncating).unwrap();
    assert_eq!(t.write(fd, b"renamed"), Ok(7));
    block_on(kernel.close(&t.caller, fd)).unwrap();
    let fd = t.open("/proc/self/comm", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read_all(fd), "renamed\n");

    let name = path(&memory, "/proc/self/comm");
    assert_eq!(
        block_on(kernel.fchmodat(&t.caller, CWD, name, 0o666)),
        Err(Errno::EPERM.into())
    );
    assert_eq!(
        t.stat("/proc/self/comm", AtFlags(0)).unwrap().stx_mode & 0o777,
        0o644
    );
}

/// `kernel.yama.ptrace_scope` starts at Yama's 1, takes 0 to 3 from a writer with
/// `CAP_SYS_PTRACE` (`yama_dointvec_minmax`), and once 3 keeps it.
#[test]
fn yama_ptrace_scope_takes_0_to_3_and_locks_at_3() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let scope = "/proc/sys/kernel/yama/ptrace_scope";
    let set = |value: &str| {
        let fd = t.open(scope, OpenFlags::O_WRONLY)?;
        let written = t.write(fd, value.as_bytes());
        block_on(kernel.close(&t.caller, fd)).unwrap();
        written
    };
    let get = || {
        let fd = t.open(scope, OpenFlags::O_RDONLY).unwrap();
        t.read_all(fd)
    };

    assert_eq!(get(), "1\n");
    let mode = t.stat(scope, AtFlags::empty()).unwrap().stx_mode;
    assert_eq!(mode & 0o7777, 0o644);
    assert_eq!(set("0\n"), Ok(2));
    assert_eq!(get(), "0\n");
    assert_eq!(set("4"), Err(Errno::EINVAL.into()));

    assert_eq!(set("3"), Ok(1));
    assert_eq!(set("1"), Err(Errno::EINVAL.into()));
    assert_eq!(get(), "3\n");

    let all = u64::MAX >> (63 - u64::from(wasixv2_vfs::Cap::LAST));
    let sys_ptrace = wasixv2_vfs::Cap::SysPtrace.bit();
    super::cred::set_caps(&kernel, &t.caller, &memory, all & !sys_ptrace);
    assert_eq!(set("3"), Err(Errno::EPERM.into()));
}

/// The `/proc/<pid>` files of the OOM score, audit identity, scheduling, I/O and memory: what
/// they show and what writes they take.
#[test]
fn pid_files_for_oom_audit_sched_io_and_memory() {
    let (kernel, task, _) = super::cred::machine_as(0, 0);
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let read = |name: &str| {
        let fd = t.open(&format!("/proc/self/{name}"), OpenFlags::O_RDONLY)?;
        let text = t.read_all(fd);
        block_on(kernel.close(&t.caller, fd)).unwrap();
        Ok::<_, SysErr>(text)
    };
    let set = |name: &str, value: &str| {
        let fd = t.open(&format!("/proc/self/{name}"), OpenFlags::O_WRONLY)?;
        let written = t.write(fd, value.as_bytes());
        block_on(kernel.close(&t.caller, fd)).unwrap();
        written
    };

    assert_eq!(read("oom_score_adj").unwrap(), "0\n");
    assert!(read("oom_score").unwrap().starts_with("10"));
    assert_eq!(set("oom_score_adj", "500\n"), Ok(4));
    assert_eq!(read("oom_adj").unwrap(), "8\n");
    assert_eq!(set("oom_score_adj", "1001"), Err(Errno::EINVAL.into()));
    assert_eq!(set("oom_adj", "-17"), Ok(3));
    assert_eq!(read("oom_score_adj").unwrap(), "-1000\n");
    assert_eq!(read("oom_score").unwrap(), "0\n", "never picked");
    assert_eq!(set("oom_adj", "16"), Err(Errno::EINVAL.into()));

    assert_eq!(read("personality").unwrap(), "00000000\n");
    assert_eq!(read("wchan").unwrap(), "0");
    assert_eq!(read("stack").unwrap(), "");
    assert_eq!(read("timerslack_ns").unwrap(), "50000\n");
    assert_eq!(set("timerslack_ns", "1000"), Ok(4));
    assert_eq!(read("timerslack_ns").unwrap(), "1000\n");
    assert_eq!(set("timerslack_ns", "0"), Ok(1));
    assert_eq!(read("timerslack_ns").unwrap(), "50000\n");
    assert_eq!(read("schedstat").unwrap().split(' ').count(), 3);
    assert!(read("sched").unwrap().starts_with("test ("));
    assert!(
        read("smaps_rollup")
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .ends_with("[rollup]")
    );
    assert!(
        read("mountstats")
            .unwrap()
            .contains(" mounted on /proc with fstype proc\n")
    );

    // The audit identity: unset until set, then a new session.
    assert_eq!(read("loginuid").unwrap(), "4294967295");
    assert_eq!(read("sessionid").unwrap(), "4294967295");
    assert_eq!(set("loginuid", "1000"), Ok(4));
    assert_eq!(
        (read("loginuid").unwrap(), read("sessionid").unwrap()),
        ("1000".into(), "1".into())
    );
    let all = u64::MAX >> (63 - u64::from(wasixv2_vfs::Cap::LAST));
    let audit = wasixv2_vfs::Cap::AuditControl.bit() | wasixv2_vfs::Cap::SysResource.bit();
    super::cred::set_caps(&kernel, &t.caller, &memory, all & !audit);
    assert_eq!(set("loginuid", "1001"), Err(Errno::EPERM.into()));
    // The capable write of 500 set the floor; only the legacy file compares with the value.
    assert_eq!(set("oom_score_adj", "-1000"), Err(Errno::EACCES.into()));
    assert_eq!(set("oom_adj", "-17"), Ok(3));
    assert_eq!(set("oom_score_adj", "600"), Ok(3));
    assert_eq!(
        set("oom_adj", "-1"),
        Err(Errno::EACCES.into()),
        "below the value"
    );

    let io = read("io").unwrap();
    let count = |key: &str| -> u64 {
        let line = io.lines().find(|l| l.starts_with(key)).unwrap();
        line.split_whitespace().nth(1).unwrap().parse().unwrap()
    };
    assert!(count("wchar:") >= 10 && count("syscw:") >= 5, "{io}");
    assert!(count("rchar:") > 0 && count("syscr:") > 0);
}

/// `/proc/pressure`, `modules`, `kallsyms`, `timer_list`, `buddyinfo`, `zoneinfo` and `crypto`,
/// and the inotify limits: `fs.inotify.max_user_*` are the initial namespace's
/// `user.max_inotify_*`, which `inotify_init1` (`EMFILE`) and `inotify_add_watch` (`ENOSPC`)
/// enforce.
#[test]
fn system_files_and_inotify_limits() {
    let (kernel, task, _) =
        super::cred::machine_with(|kernel| kernel.with_credentials(Credentials::new(0, 0)));
    let procfs = kernel.procfs();
    let mount = kernel
        .vfs()
        .mount(b"/proc", procfs, MountFlags::default(), true);
    block_on(mount).unwrap();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let read = |at: &str| {
        let fd = t.open(at, OpenFlags::O_RDONLY).unwrap();
        t.read_all(fd)
    };
    let none = "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n\
                full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
    for kind in ["cpu", "io", "memory"] {
        assert_eq!(read(&format!("/proc/pressure/{kind}")), none);
    }
    assert_eq!(read("/proc/modules"), "");
    assert_eq!(read("/proc/kallsyms"), "");
    assert!(read("/proc/timer_list").starts_with("Timer List Version: v0.10\n"));
    let buddy = read("/proc/buddyinfo");
    assert!(buddy.starts_with("Node 0, zone   Normal "), "{buddy}");
    assert_eq!(buddy.split_whitespace().count(), 4 + 11, "{buddy}");
    assert!(read("/proc/zoneinfo").contains("  pages free     "));
    assert!(read("/proc/crypto").contains("driver       : sha256-generic\n"));

    assert_eq!(read("/proc/sys/fs/inotify/max_user_instances"), "128\n");
    assert_eq!(read("/proc/sys/user/max_inotify_instances"), "128\n");
    let watches: u32 = read("/proc/sys/fs/inotify/max_user_watches")
        .trim()
        .parse()
        .unwrap();
    assert!((8192..=1_048_576).contains(&watches), "{watches}");
    let fd = t
        .open(
            "/proc/sys/fs/inotify/max_user_instances",
            OpenFlags::O_WRONLY,
        )
        .unwrap();
    assert_eq!(t.write(fd, b"1\n"), Ok(2));
    let fd = t
        .open("/proc/sys/fs/inotify/max_user_watches", OpenFlags::O_WRONLY)
        .unwrap();
    assert_eq!(t.write(fd, b"1\n"), Ok(2));
    let caller = Caller::new(task, &memory);
    let flags = wasixv2_abi::InotifyInitFlags(0);
    let inotify = block_on(kernel.inotify_init1(&caller, flags)).unwrap();
    assert_eq!(
        errno(block_on(kernel.inotify_init1(&caller, flags))),
        Errno::EMFILE
    );
    let watch = |at: &str| {
        let at = path(&memory, at);
        block_on(kernel.inotify_add_watch(
            &caller,
            inotify,
            at,
            wasixv2_abi::InotifyMask::IN_MODIFY,
        ))
    };
    assert!(watch("/proc").is_ok());
    assert_eq!(errno(watch("/")), Errno::ENOSPC);
    block_on(kernel.close(&caller, inotify)).unwrap();
    assert!(
        block_on(kernel.inotify_init1(&caller, flags)).is_ok(),
        "the count went with it"
    );
}

/// fs/proc/base.c `proc_pid_syscall`: show blocked syscall arguments and ptrace access rules.
#[test]
fn syscall_file_shows_running_stopped_and_blocked_calls() {
    let (kernel, parent, _) = machine();
    let memory = super::proc::ForkableMemory(memory());
    let caller = Caller::new(parent, &memory);
    let child = super::proc::fork_child(&kernel, &caller);
    let child_memory = self::memory();
    let child_caller = Caller::new(child, &child_memory);
    let t = T::new(&kernel, parent, &memory.0);
    let name = format!("/proc/{}/syscall", child.0);
    let read = || {
        let fd = t.open(&name, OpenFlags::O_RDONLY).unwrap();
        t.read_all(fd)
    };
    assert_eq!(read(), "running\n");
    let (r, _) = pipe(&kernel, &child_caller, OpenFlags(0));
    let stack = wasixv2_syscalls::Layered::new(SyscallEntry(Dispatch), kernel);
    let mut blocked = Box::pin(stack.read(&child_caller, r, UserSliceMut::new(0x800, 3)));
    assert!(blocked.as_mut().now_or_never().is_none());
    let t = T::new(&stack.inner, parent, &memory.0);
    let fd = t.open(&name, OpenFlags::O_RDONLY).unwrap();
    assert_eq!(
        t.read_all(fd),
        format!("63 0x{:x} 0x800 0x3 0x0 0x0 0x0 0x0 0x0\n", r.0)
    );
    drop(blocked);
    let fd = t.open(&name, OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read_all(fd), "running\n");
    stack
        .inner
        .kill(child, wasixv2_abi::Signal::SIGSTOP)
        .unwrap();
    assert_eq!(stack.inner.take_pending(child), Some(GuestAction::Stopped));
    let fd = t.open(&name, OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read_all(fd), "-1 0x0 0x0\n");
    let sibling = super::proc::fork_child(&stack.inner, &caller);
    let sibling_memory = self::memory();
    let sibling_t = T::new(&stack.inner, sibling, &sibling_memory);
    assert_eq!(
        errno(sibling_t.open(&name, OpenFlags::O_RDONLY)),
        Errno::EPERM
    );
}
