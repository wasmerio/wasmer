//! Filesystem syscalls on open files and paths: `openat2` scoping, locks, allocation, copies,
//! status flags and the generic ioctls.

use wasixv2_abi::ResolveFlags;

use super::ipc::{errno, get, machine, put};
use super::*;

const CWD: Fd = Fd(-100);

/// One task's filesystem calls, with its memory at hand. Paths go at 0x1000, data at 0x2000.
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

    fn path(&self, path: &str) -> Cstr {
        let mut bytes = path.as_bytes().to_vec();
        bytes.push(0);
        put(self.memory, 0x1000, &bytes);
        GuestPtr::new(0x1000)
    }

    fn open_at(&self, dfd: Fd, at: &str, flags: OpenFlags) -> SysResult<Fd> {
        let name = self.path(at);
        block_on(self.kernel.openat(&self.caller, dfd, name, flags, 0o644))
    }

    fn open(&self, at: &str, flags: OpenFlags) -> SysResult<Fd> {
        self.open_at(CWD, at, flags)
    }

    fn openat2(&self, dfd: Fd, at: &str, flags: OpenFlags, resolve: ResolveFlags) -> SysResult<Fd> {
        let how: Vec<u8> = [u64::from(flags.0), 0, resolve.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        put(self.memory, 0x1800, &how);
        let name = self.path(at);
        block_on(
            self.kernel
                .openat2(&self.caller, dfd, name, GuestPtr::new(0x1800), 24),
        )
    }

    fn mkdir(&self, at: &str) {
        let name = self.path(at);
        assert_eq!(
            block_on(self.kernel.mkdirat(&self.caller, CWD, name, 0o755)),
            Ok(0)
        );
    }

    /// Creates `at` holding `data`.
    fn create(&self, at: &str, data: &[u8]) {
        let fd = self
            .open(
                at,
                OpenFlags::O_CREAT | OpenFlags::O_WRONLY | OpenFlags::O_TRUNC,
            )
            .unwrap();
        assert_eq!(self.write(fd, data), Ok(data.len() as u32));
        self.close(fd);
    }

    fn symlink(&self, target: &str, at: &str) {
        let mut bytes = target.as_bytes().to_vec();
        bytes.push(0);
        put(self.memory, 0x1400, &bytes);
        let name = self.path(at);
        let link = self
            .kernel
            .symlinkat(&self.caller, GuestPtr::new(0x1400), CWD, name);
        assert_eq!(block_on(link), Ok(0));
    }

    fn write(&self, fd: Fd, data: &[u8]) -> SysResult<u32> {
        put(self.memory, 0x2000, data);
        let buf = UserSlice::new(0x2000, data.len() as u32);
        block_on(self.kernel.write(&self.caller, fd, buf))
    }

    fn read(&self, fd: Fd, len: u32) -> SysResult<Vec<u8>> {
        let buf = UserSliceMut::new(0x2000, len);
        let n = block_on(self.kernel.read(&self.caller, fd, buf))?;
        Ok(get(self.memory, 0x2000, n as usize))
    }

    fn close(&self, fd: Fd) {
        assert_eq!(block_on(self.kernel.close(&self.caller, fd)), Ok(0));
    }
}

#[test]
fn resolve_in_root_walks_absolute_paths_and_dotdot_from_dfd() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    for dir in ["/jail", "/jail/etc", "/jail/sub", "/etc"] {
        t.mkdir(dir);
    }
    t.create("/etc/passwd", b"host");
    t.create("/jail/etc/passwd", b"jail");
    t.create("/notdir", b"");
    t.symlink("/etc/passwd", "/jail/abs");
    t.symlink("../../etc/passwd", "/jail/sub/up");
    let jail = t
        .open("/jail", OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY)
        .unwrap();
    let in_root = ResolveFlags::RESOLVE_IN_ROOT;
    let read = |at: &str| {
        let fd = t.openat2(jail, at, OpenFlags::O_RDONLY, in_root)?;
        let data = t.read(fd, 16);
        t.close(fd);
        data
    };

    // `/`, `..` above the root and absolute symlinks all stay in the jail.
    assert_eq!(read("/etc/passwd"), Ok(b"jail".to_vec()));
    assert_eq!(read("//etc/../../etc/passwd"), Ok(b"jail".to_vec()));
    assert_eq!(read("abs"), Ok(b"jail".to_vec()));
    assert_eq!(read("sub/up"), Ok(b"jail".to_vec()));
    // The descriptor is checked even for an absolute path, as Linux's `path_init` does.
    let bad = t.openat2(Fd(99), "/etc/passwd", OpenFlags::O_RDONLY, in_root);
    assert_eq!(errno(bad), Errno::EBADF);
    let file = t.open("/notdir", OpenFlags::O_RDONLY).unwrap();
    let not_dir = t.openat2(file, "/", OpenFlags::O_RDONLY, in_root);
    assert_eq!(errno(not_dir), Errno::ENOTDIR);
    // Without a descriptor the root is the working directory.
    let name = t.path("/jail");
    assert_eq!(block_on(kernel.chdir(&t.caller, name)), Ok(0));
    let fd = t.openat2(CWD, "/etc/passwd", OpenFlags::O_RDONLY, in_root);
    assert_eq!(t.read(fd.unwrap(), 16), Ok(b"jail".to_vec()));
}

#[test]
fn a_posix_lock_won_after_its_descriptor_closed_is_released() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.create("/l", b"");
    let open = || t.open("/l", OpenFlags::O_RDWR).unwrap();
    let (waiter, holder, probe) = (open(), open(), open());
    // flock64 { l_type = F_WRLCK, l_whence = SEEK_SET, l_start = 0, l_len = 0 } at 0x3000.
    put(&memory, 0x3000, &[1, 0, 0, 0]);
    let lock = |fd, cmd| block_on(kernel.fcntl64(&t.caller, fd, cmd, 0x3000));
    assert_eq!(lock(holder, FcntlCmd::F_OFD_SETLK), Ok(0));

    // While `waiter` waits for the OFD lock, another thread closes it and then the holder.
    let pid = kernel.process(task).pid;
    let set = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            let closed = kernel.files(task).lock().unwrap().remove(waiter).unwrap();
            kernel.closed_by(pid, &closed);
            drop(kernel.files(task).lock().unwrap().remove(holder));
        });
        lock(waiter, FcntlCmd::F_SETLKW64)
    });

    // `fcntl_setlk` notices the close and drops the lock it won, which nobody could release.
    assert_eq!(errno(set), Errno::EBADF);
    assert_eq!(lock(probe, FcntlCmd::F_OFD_GETLK), Ok(0));
    assert_eq!(get(&memory, 0x3000, 2), [2, 0], "F_UNLCK");
}

#[test]
fn pipes_are_flocked_through_their_inode() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let (r, w) = super::ipc::pipe(&kernel, &t.caller, OpenFlags(0));
    let flock = |fd, op| block_on(kernel.flock(&t.caller, fd, op));
    let try_ex = FlockOp::LOCK_EX | FlockOp::LOCK_NB;

    // Both ends are one inode, as on Linux, so their locks conflict.
    assert_eq!(flock(r, FlockOp::LOCK_EX), Ok(0));
    assert_eq!(errno(flock(w, try_ex)), Errno::EWOULDBLOCK);
    let try_sh = FlockOp::LOCK_SH | FlockOp::LOCK_NB;
    assert_eq!(errno(flock(w, try_sh)), Errno::EWOULDBLOCK);
    // The last close of the locked end releases its lock.
    t.close(r);
    assert_eq!(flock(w, try_ex), Ok(0));
    assert_eq!(flock(w, FlockOp::LOCK_UN), Ok(0));
}

#[test]
fn fallocate_punches_holes_through_an_append_descriptor() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.create("/f", &[1; 8192]);
    let fd = t
        .open("/f", OpenFlags::O_RDWR | OpenFlags::O_APPEND)
        .unwrap();
    // `vfs_fallocate` checks the inode's append-only flag, not the descriptor's O_APPEND.
    let punch = FallocFlags::FALLOC_FL_PUNCH_HOLE | FallocFlags::FALLOC_FL_KEEP_SIZE;
    assert_eq!(
        block_on(kernel.fallocate(&t.caller, fd, punch, 0, 4096)),
        Ok(0)
    );
    let zero = FallocFlags::FALLOC_FL_ZERO_RANGE;
    assert_eq!(
        block_on(kernel.fallocate(&t.caller, fd, zero, 4096, 4096)),
        Ok(0)
    );
    let fd = t.open("/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read(fd, 8192), Ok(vec![0; 8192]));
}

#[test]
fn sendfile_reads_sockets_without_an_offset_and_never_pipes() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let out = t
        .open("/out", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    let pair = wasixv2_abi::FdPair::new(0x100);
    let unix = wasixv2_abi::AddressFamily(1);
    const SOCK_STREAM: i32 = 1;
    let made = kernel.socketpair(&t.caller, unix, SOCK_STREAM, 0, pair);
    assert_eq!(block_on(made), Ok(0));
    let fd = |at| Fd(i32::from_le_bytes(get(&memory, at, 4).try_into().unwrap()));
    let (a, b) = (fd(0x100), fd(0x104));
    let (r, _w) = super::ipc::pipe(&kernel, &t.caller, OpenFlags(0));
    put(&memory, 0x3000, &0i64.to_le_bytes());
    let sendfile = |input, offset: u32, count| {
        let offset = wasixv2_abi::LoffPtr::new(offset);
        block_on(kernel.sendfile64(&t.caller, out, input, offset, count))
    };

    // `do_sendfile`: an offset needs FMODE_PREAD (ESPIPE); `do_splice_direct` needs an
    // input with `splice_read`, which sockets have and pipes do not (EINVAL).
    assert_eq!(errno(sendfile(b, 0x3000, 5)), Errno::ESPIPE);
    assert_eq!(errno(sendfile(r, 0x3000, 5)), Errno::ESPIPE);
    assert_eq!(errno(sendfile(r, 0, 5)), Errno::EINVAL);
    assert_eq!(t.write(a, b"hello"), Ok(5));
    assert_eq!(sendfile(b, 0, 5), Ok(5));
    let back = t.open("/out", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read(back, 16), Ok(b"hello".to_vec()));
}

/// The `(wd, mask, name)` of every event queued on the inotify instance `fd`.
fn inotify_events(t: &T<'_>, fd: Fd) -> Vec<(i32, u32, String)> {
    let Ok(raw) = t.read(fd, 4096) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        let word = |i: usize| u32::from_ne_bytes(raw[at + i..at + i + 4].try_into().unwrap());
        let len = word(12) as usize;
        let name = &raw[at + 16..at + 16 + len];
        let name = name.split(|&b| b == 0).next().unwrap();
        events.push((
            word(0) as i32,
            word(4),
            String::from_utf8(name.to_vec()).unwrap(),
        ));
        at += 16 + len;
    }
    events
}

#[test]
fn inotify_takes_flag_only_masks_and_honours_excl_unlink() {
    use wasixv2_abi::{InotifyInitFlags, InotifyMask};
    const IN_EXCL_UNLINK: u32 = 0x0400_0000;
    const FS_EVENT_ON_CHILD: u32 = 0x0800_0000;

    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    t.create("/d/f", b"");
    let init = || block_on(kernel.inotify_init1(&t.caller, InotifyInitFlags::IN_NONBLOCK)).unwrap();
    let watch = |fd, at: &str, mask: u32| {
        let name = t.path(at);
        let add = kernel.inotify_add_watch(&t.caller, fd, name, InotifyMask(mask));
        block_on(add)
    };
    let (plain, excl) = (init(), init());

    // `ALL_INOTIFY_BITS`: flags alone make a watch; bits outside inotify's are EINVAL.
    let dont_follow = InotifyMask::IN_DONT_FOLLOW.0;
    assert_eq!(watch(plain, "/d/f", dont_follow), Ok(1));
    let modify = InotifyMask::IN_MODIFY.0;
    assert_eq!(
        errno(watch(plain, "/d", modify | FS_EVENT_ON_CHILD)),
        Errno::EINVAL
    );

    // A file written after its name is gone: the directory it was in still hears of it, unless
    // the watch asked for IN_EXCL_UNLINK.
    assert_eq!(watch(plain, "/d", modify), Ok(2));
    assert_eq!(watch(excl, "/d", modify | IN_EXCL_UNLINK), Ok(1));
    let fd = t.open("/d/f", OpenFlags::O_WRONLY).unwrap();
    let name = t.path("/d/f");
    let unlink = kernel.unlinkat(&t.caller, CWD, name, AtFlags(0));
    assert_eq!(block_on(unlink), Ok(0));
    inotify_events(&t, plain);
    assert_eq!(t.write(fd, b"x"), Ok(1));
    assert_eq!(inotify_events(&t, plain), [(2, modify, "f".to_string())]);
    assert_eq!(inotify_events(&t, excl), []);
}

#[test]
fn copy_file_range_stops_at_the_input_end_before_checking_overlap() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.create("/f", b"0123456789");
    let fd = t.open("/f", OpenFlags::O_RDWR).unwrap();
    let copy = |pos_in: i64, pos_out: i64, len| {
        put(&memory, 0x3000, &pos_in.to_le_bytes());
        put(&memory, 0x3008, &pos_out.to_le_bytes());
        let (off_in, off_out) = (LoffPtr::new(0x3000), LoffPtr::new(0x3008));
        block_on(kernel.copy_file_range(&t.caller, fd, off_in, fd, off_out, len, 0))
    };

    // Shortened to the 10 bytes there are, the copy to offset 10 does not overlap its source.
    assert_eq!(copy(0, 10, 100), Ok(10));
    assert_eq!(t.read(fd, 32), Ok(b"01234567890123456789".to_vec()));
    // Ranges that still overlap after shortening are EINVAL.
    assert_eq!(errno(copy(0, 5, 10)), Errno::EINVAL);
    // At the largest file offset, nothing can be written: EFBIG.
    assert_eq!(errno(copy(0, i64::MAX, 1)), Errno::EFBIG);
    // From the input's end, nothing is copied.
    assert_eq!(copy(20, 0, 5), Ok(0));
}
