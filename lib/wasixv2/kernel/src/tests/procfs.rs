//! `/proc` and the character devices: magic links, the pseudo inodes behind pipes and event
//! descriptors, and device semantics.

use futures::FutureExt as _;
use wasixv2_abi::{EventfdFlags, ResolveFlags, Statx};
use wasixv2_syscalls::read_value;
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
