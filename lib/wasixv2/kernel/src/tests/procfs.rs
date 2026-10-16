//! `/proc` and the character devices: magic links, the pseudo inodes behind pipes and event
//! descriptors, and device semantics.

use wasixv2_abi::{EventfdFlags, ResolveFlags, Statx};
use wasixv2_syscalls::read_value;
use wasixv2_vfs::{FileType, MountFlags};

use super::ipc::{errno, get, pipe, put};
use super::*;

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
fn opening_stdouts_fd_link_reaches_the_same_stream() {
    let (kernel, task, stdout) = machine();
    let memory = memory();
    let t = T::new(&kernel, task, &memory);
    let fd = t.open("/proc/self/fd/1", OpenFlags::O_WRONLY).unwrap();

    assert_eq!(t.write(fd, b"out"), Ok(3));
    assert_eq!(*stdout.0.lock().unwrap(), b"out");
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
