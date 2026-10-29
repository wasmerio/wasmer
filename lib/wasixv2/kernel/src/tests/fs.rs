//! Filesystem syscalls on open files and paths: `openat2` scoping, locks, allocation, copies,
//! status flags and the generic ioctls.

use wasixv2_abi::{EventfdFlags, FileHandlePtr, ResolveFlags, S32Ptr, SpliceFlags};

use super::ipc::{errno, get, machine, put};
use super::*;
use crate::sync::MutexExt;

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
            let closed = kernel.files(task).lock_or_recover().remove(waiter).unwrap();
            kernel.closed_by(pid, &closed);
            drop(kernel.files(task).lock_or_recover().remove(holder));
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

#[test]
fn getfl_keeps_open_flags_and_setfl_takes_o_direct_only_where_it_works() {
    use wasixv2_abi::EventfdFlags;

    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    let getfl = |fd| block_on(kernel.fcntl64(&t.caller, fd, FcntlCmd::F_GETFL, 0)).unwrap() as u32;
    let setfl =
        |fd, flags: OpenFlags| block_on(kernel.fcntl64(&t.caller, fd, FcntlCmd::F_SETFL, flags.0));

    // `f_flags` is the open flags but O_CREAT, O_EXCL, O_NOCTTY, O_TRUNC and O_CLOEXEC.
    let dir_flags = OpenFlags::O_DIRECTORY | OpenFlags::O_NOFOLLOW;
    let dir = t.open("/d", dir_flags | OpenFlags::O_CLOEXEC).unwrap();
    assert_eq!(OpenFlags(getfl(dir)), dir_flags);
    let created = OpenFlags::O_CREAT | OpenFlags::O_EXCL | OpenFlags::O_TRUNC;
    let file = t.open("/f", created | OpenFlags::O_RDWR).unwrap();
    assert_eq!(OpenFlags(getfl(file)), OpenFlags::O_RDWR);

    // `setfl`: O_DIRECT needs a pipe or FMODE_CAN_ODIRECT.
    assert_eq!(setfl(file, OpenFlags::O_DIRECT), Ok(0));
    assert!(OpenFlags(getfl(file)).contains(OpenFlags::O_DIRECT));
    let (r, _w) = super::ipc::pipe(&kernel, &t.caller, OpenFlags(0));
    assert_eq!(setfl(r, OpenFlags::O_DIRECT), Ok(0));
    let event = block_on(kernel.eventfd2(&t.caller, 0, EventfdFlags(0))).unwrap();
    assert_eq!(errno(setfl(event, OpenFlags::O_DIRECT)), Errno::EINVAL);
    assert_eq!(errno(setfl(dir, OpenFlags::O_DIRECT)), Errno::EINVAL);
}

#[test]
fn created_query_reports_the_open_that_made_the_file() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    let query = |fd| block_on(kernel.fcntl64(&t.caller, fd, FcntlCmd::F_CREATED_QUERY, 0));

    let made = t
        .open("/f", OpenFlags::O_CREAT | OpenFlags::O_RDONLY)
        .unwrap();
    assert_eq!(query(made), Ok(1));
    // Found, not made: O_CREAT alone does not count.
    let found = t
        .open("/f", OpenFlags::O_CREAT | OpenFlags::O_RDONLY)
        .unwrap();
    assert_eq!(query(found), Ok(0));
    let plain = t.open("/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(query(plain), Ok(0));
    // `check_fcntl_cmd` allows it on O_PATH descriptors.
    let path = t.open("/f", OpenFlags::O_PATH).unwrap();
    assert_eq!(query(path), Ok(0));
    // `vfs_tmpfile` names nothing, so nothing counts as created.
    let tmp = t.open(
        "/d",
        OpenFlags::__O_TMPFILE | OpenFlags::O_DIRECTORY | OpenFlags::O_RDWR,
    );
    assert_eq!(query(tmp.unwrap()), Ok(0));
    let (r, _w) = super::ipc::pipe(&kernel, &t.caller, OpenFlags(0));
    assert_eq!(query(r), Ok(0));
}

#[test]
fn generic_ioctls_give_block_size_bytes_used_and_refuse_async() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.create("/f", &[1; 8192]);
    let fd = t.open("/f", OpenFlags::O_RDONLY).unwrap();
    let ioctl = |request, arg| block_on(kernel.ioctl(&t.caller, fd, request, arg));
    let word = |at| i32::from_le_bytes(get(&memory, at, 4).try_into().unwrap());

    assert_eq!(ioctl(IoctlRequest(2), 0x3000), Ok(0), "FIGETBSZ");
    assert!(word(0x3000) > 0);
    assert_eq!(ioctl(IoctlRequest::FIOQSIZE, 0x3000), Ok(0));
    let bytes = i64::from_le_bytes(get(&memory, 0x3000, 8).try_into().unwrap());
    let vfs = t.kernel.vfs().clone();
    let path = block_on(vfs.lookup(
        &wasixv2_vfs::Ctx {
            cred: &wasixv2_vfs::Cred::root(),
            root: &vfs.root(),
            umask: 0,
            task: 0,
            leases: None,
        },
        &vfs.root(),
        b"/f",
        wasixv2_vfs::LookupFlags::follow(),
    ))
    .unwrap();
    let blocks = block_on(path.dentry.node().getattr()).unwrap().blocks;
    assert_eq!(bytes, blocks as i64 * 512);
    // A regular file has no `fasync`: asking for what is already so is fine, a change is not.
    put(&memory, 0x3000, &0i32.to_le_bytes());
    assert_eq!(ioctl(IoctlRequest::FIOASYNC, 0x3000), Ok(0));
    put(&memory, 0x3000, &1i32.to_le_bytes());
    assert_eq!(errno(ioctl(IoctlRequest::FIOASYNC, 0x3000)), Errno::ENOTTY);
    let (r, _w) = super::ipc::pipe(&kernel, &t.caller, OpenFlags(0));
    let qsize = block_on(kernel.ioctl(&t.caller, r, IoctlRequest::FIOQSIZE, 0x3000));
    assert_eq!(errno(qsize), Errno::ENOTTY);
}

/// `FICLONE`, `FICLONERANGE` and `FIDEDUPERANGE` make `ioctl_file_clone`'s and
/// `vfs_dedupe_file_range`'s checks, then fail with `EOPNOTSUPP`: no filesystem here shares
/// extents (`remap_file_range`), so `cp --reflink=always` reports that, not "Not a tty".
#[test]
fn clone_and_dedupe_ioctls_check_their_files_then_are_unsupported() {
    const FICLONE: IoctlRequest = IoctlRequest(0x4004_9409);
    const FICLONERANGE: IoctlRequest = IoctlRequest(0x4020_940d);
    const FIDEDUPERANGE: IoctlRequest = IoctlRequest(0xc018_9436);
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.create("/src", b"data");
    t.mkdir("/d");
    let src = t.open("/src", OpenFlags::O_RDONLY).unwrap();
    let wronly = t.open("/src", OpenFlags::O_WRONLY).unwrap();
    let dst = t
        .open("/dst", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    let rdonly = t.open("/dst", OpenFlags::O_RDONLY).unwrap();
    let append = t
        .open("/dst", OpenFlags::O_WRONLY | OpenFlags::O_APPEND)
        .unwrap();
    let dir = t
        .open("/d", OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY)
        .unwrap();
    let (pipe_r, _pipe_w) = super::ipc::pipe(&kernel, &t.caller, OpenFlags(0));
    let ioctl = |fd, request, arg| errno(block_on(kernel.ioctl(&t.caller, fd, request, arg)));
    let clone = |dst, src: Fd| ioctl(dst, FICLONE, src.0 as u32);

    assert_eq!(clone(dst, src), Errno::EOPNOTSUPP);
    assert_eq!(clone(dst, Fd(99)), Errno::EBADF);
    assert_eq!(clone(dst, pipe_r), Errno::EXDEV);
    assert_eq!(clone(dst, dir), Errno::EISDIR);
    assert_eq!(clone(dst, wronly), Errno::EBADF);
    assert_eq!(clone(rdonly, src), Errno::EBADF);
    assert_eq!(clone(append, src), Errno::EBADF);

    let range: Vec<u8> = [i64::from(src.0), 0, 4, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    put(&memory, 0x3000, &range);
    assert_eq!(ioctl(dst, FICLONERANGE, 0x3000), Errno::EOPNOTSUPP);
    assert_eq!(ioctl(dst, FICLONERANGE, 1 << 30), Errno::EFAULT);

    // One destination: a 24-byte header and a 32-byte element.
    let mut dedupe = vec![0; 56];
    dedupe[8..16].copy_from_slice(&4u64.to_le_bytes());
    dedupe[16..18].copy_from_slice(&1u16.to_le_bytes());
    dedupe[24..32].copy_from_slice(&i64::from(dst.0).to_le_bytes());
    put(&memory, 0x3000, &dedupe);
    assert_eq!(ioctl(src, FIDEDUPERANGE, 0x3000), Errno::EOPNOTSUPP);
    assert_eq!(ioctl(wronly, FIDEDUPERANGE, 0x3000), Errno::EINVAL);
    assert_eq!(ioctl(dir, FIDEDUPERANGE, 0x3000), Errno::EISDIR);
    assert_eq!(ioctl(src, FIDEDUPERANGE, 1 << 30), Errno::EFAULT);
    put(&memory, 0x3012, &1u16.to_le_bytes());
    assert_eq!(ioctl(src, FIDEDUPERANGE, 0x3000), Errno::EINVAL);
    // More elements than a page holds.
    put(&memory, 0x3010, &200u16.to_le_bytes());
    assert_eq!(ioctl(src, FIDEDUPERANGE, 0x3000), Errno::ENOMEM);
}

const SPLICE_F_NONBLOCK: SpliceFlags = SpliceFlags::SPLICE_F_NONBLOCK;

impl T<'_> {
    /// `splice` with the offsets (if any) at 0x3000 and 0x3008.
    fn splice(
        &self,
        input: Fd,
        off_in: Option<i64>,
        output: Fd,
        off_out: Option<i64>,
        len: u32,
        flags: SpliceFlags,
    ) -> SysResult<i32> {
        let offset = |at: u32, value: Option<i64>| match value {
            Some(value) => {
                put(self.memory, at, &value.to_le_bytes());
                wasixv2_abi::LoffPtr::new(at)
            }
            None => wasixv2_abi::LoffPtr::NULL,
        };
        let (off_in, off_out) = (offset(0x3000, off_in), offset(0x3008, off_out));
        block_on(
            self.kernel
                .splice(&self.caller, input, off_in, output, off_out, len, flags),
        )
    }

    fn offset(&self, at: u32) -> i64 {
        i64::from_le_bytes(get(self.memory, at, 8).try_into().unwrap())
    }

    fn pipe(&self, flags: OpenFlags) -> (Fd, Fd) {
        super::ipc::pipe(self.kernel, &self.caller, flags)
    }
}

/// `do_splice` from a file into a pipe and from a pipe into a file: at the caller's offset,
/// which is written back while the file position stays, or at the file position, which moves.
#[test]
fn splice_moves_file_data_through_a_pipe_at_offsets_or_the_position() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.create("/in", b"hello world");
    let input = t.open("/in", OpenFlags::O_RDONLY).unwrap();
    let output = t
        .open("/out", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    let (r, w) = t.pipe(OpenFlags(0));

    assert_eq!(
        t.splice(input, Some(6), w, None, 100, SpliceFlags(0)),
        Ok(5)
    );
    assert_eq!(t.offset(0x3000), 11);
    assert_eq!(t.splice(input, None, w, None, 5, SpliceFlags(0)), Ok(5));
    // At the end of the file: nothing.
    assert_eq!(t.splice(input, Some(11), w, None, 5, SpliceFlags(0)), Ok(0));
    assert_eq!(t.read(input, 16), Ok(b" world".to_vec()));

    // Out of the pipe, only what it holds: "worldhello".
    assert_eq!(
        t.splice(r, None, output, Some(2), 100, SpliceFlags(0)),
        Ok(10)
    );
    assert_eq!(t.offset(0x3008), 12);
    assert_eq!(t.write(w, b"!"), Ok(1));
    assert_eq!(t.splice(r, None, output, None, 100, SpliceFlags(0)), Ok(1));
    let back = t.open("/out", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read(back, 16), Ok(b"!\0worldhello".to_vec()));
    // The pipe is empty: non-blocking, EAGAIN; without writers, the end.
    let empty = t.splice(r, None, output, None, 1, SPLICE_F_NONBLOCK);
    assert_eq!(errno(empty), Errno::EAGAIN);
    t.close(w);
    assert_eq!(t.splice(r, None, output, None, 1, SpliceFlags(0)), Ok(0));
}

/// `__do_splice` and `do_splice`'s refusals, in their order.
#[test]
fn splice_checks_flags_descriptors_offsets_and_ends() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    t.create("/f", b"data");
    let file = t.open("/f", OpenFlags::O_RDWR).unwrap();
    let append = t
        .open("/f", OpenFlags::O_WRONLY | OpenFlags::O_APPEND)
        .unwrap();
    let (r, w) = t.pipe(OpenFlags::O_NONBLOCK);
    let none = SpliceFlags(0);

    assert_eq!(t.splice(file, None, w, None, 0, SpliceFlags(16)), Ok(0));
    assert_eq!(
        errno(t.splice(file, None, w, None, 1, SpliceFlags(16))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(t.splice(Fd(99), None, w, None, 1, none)),
        Errno::EBADF
    );
    assert_eq!(
        errno(t.splice(r, Some(0), file, None, 1, none)),
        Errno::ESPIPE
    );
    assert_eq!(
        errno(t.splice(file, None, w, Some(0), 1, none)),
        Errno::ESPIPE
    );
    assert_eq!(errno(t.splice(w, None, file, None, 1, none)), Errno::EBADF);
    assert_eq!(errno(t.splice(file, None, r, None, 1, none)), Errno::EBADF);
    assert_eq!(
        errno(t.splice(file, None, file, None, 1, none)),
        Errno::EINVAL
    );
    assert_eq!(errno(t.splice(r, None, w, None, 1, none)), Errno::EINVAL);
    assert_eq!(
        errno(t.splice(r, None, append, None, 1, none)),
        Errno::EINVAL
    );
    assert_eq!(
        errno(t.splice(file, Some(-1), w, None, 1, none)),
        Errno::EINVAL
    );
    let bad = wasixv2_abi::LoffPtr::new(1 << 30);
    let fault = kernel.splice(&t.caller, file, bad, w, wasixv2_abi::LoffPtr::NULL, 1, none);
    assert_eq!(errno(block_on(fault)), Errno::EFAULT);
    // An eventfd has no `splice_read` or `splice_write`.
    let event = block_on(kernel.eventfd2(&t.caller, 0, EventfdFlags(0))).unwrap();
    assert_eq!(
        errno(t.splice(event, None, w, None, 1, none)),
        Errno::EINVAL
    );
    assert_eq!(t.write(w, b"x"), Ok(1));
    assert_eq!(
        errno(t.splice(r, None, event, None, 1, none)),
        Errno::EINVAL
    );

    // No reader left: EPIPE and SIGPIPE (blocked, to be seen pending).
    kernel.task_signals(task).replace_blocked(1 << 12);
    t.close(r);
    assert_eq!(errno(t.splice(file, None, w, None, 1, none)), Errno::EPIPE);
    assert!(super::net::sigpipe_pending(&kernel, &t.caller, &memory));
}

/// Pipe to pipe: `splice` moves the data, `tee` copies it and leaves it; both wait for data
/// and for room, or fail with `EAGAIN` when either end is non-blocking.
#[test]
fn splice_and_tee_between_pipes() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let (a_r, a_w) = t.pipe(OpenFlags::O_NONBLOCK);
    let (b_r, b_w) = t.pipe(OpenFlags::O_NONBLOCK);
    let tee =
        |input, output, len, flags| block_on(kernel.tee(&t.caller, input, output, len, flags));

    assert_eq!(
        errno(t.splice(a_r, None, b_w, None, 4, SpliceFlags(0))),
        Errno::EAGAIN
    );
    assert_eq!(errno(tee(a_r, b_w, 4, SpliceFlags(0))), Errno::EAGAIN);
    assert_eq!(t.write(a_w, b"abcdef"), Ok(6));
    assert_eq!(tee(a_r, b_w, 4, SpliceFlags(0)), Ok(4));
    assert_eq!(t.read(b_r, 16), Ok(b"abcd".to_vec()));
    assert_eq!(t.splice(a_r, None, b_w, None, 4, SpliceFlags(0)), Ok(4));
    assert_eq!(t.read(b_r, 16), Ok(b"abcd".to_vec()));
    assert_eq!(t.read(a_r, 16), Ok(b"ef".to_vec()));
    // `tee` needs two different pipes, read from the first and written to the second.
    assert_eq!(errno(tee(a_r, a_w, 1, SpliceFlags(0))), Errno::EINVAL);
    assert_eq!(errno(tee(a_w, b_w, 1, SpliceFlags(0))), Errno::EBADF);
    assert_eq!(tee(a_r, b_w, 0, SpliceFlags(0)), Ok(0));
    assert_eq!(errno(tee(a_r, b_w, 1, SpliceFlags(16))), Errno::EINVAL);

    // A full output: EAGAIN; no writer on an empty input: the end.
    let fcntl = |fd, cmd, arg| block_on(kernel.fcntl64(&t.caller, fd, cmd, arg));
    assert_eq!(fcntl(b_w, FcntlCmd::F_SETPIPE_SZ, 4096), Ok(4096));
    put(&memory, 0x2000, &[7; 4096]);
    let filled = block_on(kernel.write(&t.caller, b_w, UserSlice::new(0x2000, 4096)));
    assert_eq!(filled, Ok(4096));
    assert_eq!(t.write(a_w, b"x"), Ok(1));
    let full = t.splice(a_r, None, b_w, None, 1, SpliceFlags(0));
    assert_eq!(errno(full), Errno::EAGAIN);
    assert_eq!(errno(tee(a_r, b_w, 1, SpliceFlags(0))), Errno::EAGAIN);
    assert_eq!(t.read(a_r, 1), Ok(b"x".to_vec()));
    // `opipe_prep` comes before the input's end is noticed.
    t.close(a_w);
    let full = t.splice(a_r, None, b_w, None, 1, SpliceFlags(0));
    assert_eq!(errno(full), Errno::EAGAIN);
    assert_eq!(t.read(b_r, 4096).map(|data| data.len()), Ok(4096));
    assert_eq!(t.splice(a_r, None, b_w, None, 1, SpliceFlags(0)), Ok(0));
    assert_eq!(tee(a_r, b_w, 1, SpliceFlags(0)), Ok(0));
}

/// `vmsplice`: into a pipe the descriptor writes, as much as fits; out of one it only reads,
/// as a read; any other descriptor is `EBADF`.
#[test]
fn vmsplice_copies_user_buffers_into_and_out_of_pipes() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let (r, w) = t.pipe(OpenFlags(0));
    let iovec = |at: u32, base: u32, len: u32| {
        put(&memory, at, &base.to_le_bytes());
        put(&memory, at + 4, &len.to_le_bytes());
    };
    let vmsplice =
        |fd, n, flags| block_on(kernel.vmsplice(&t.caller, fd, GuestPtr::new(0x80), n, flags));
    put(&memory, 0x400, b"hello");
    put(&memory, 0x500, b" world");
    iovec(0x80, 0x400, 5);
    iovec(0x88, 0x500, 6);

    assert_eq!(vmsplice(w, 2, SpliceFlags(0)), Ok(11));
    assert_eq!(vmsplice(w, 0, SpliceFlags(0)), Ok(0));
    iovec(0x80, 0x600, 3);
    iovec(0x88, 0x700, 8);
    assert_eq!(vmsplice(r, 2, SpliceFlags(0)), Ok(11));
    assert_eq!(get(&memory, 0x600, 3), b"hel");
    assert_eq!(get(&memory, 0x700, 8), b"lo world");
    // Only SPLICE_F_NONBLOCK keeps it from waiting.
    assert_eq!(errno(vmsplice(r, 1, SPLICE_F_NONBLOCK)), Errno::EAGAIN);
    assert_eq!(errno(vmsplice(r, 1, SpliceFlags(32))), Errno::EINVAL);
    t.create("/f", b"");
    let file = t.open("/f", OpenFlags::O_RDWR).unwrap();
    assert_eq!(errno(vmsplice(file, 1, SpliceFlags(0))), Errno::EBADF);
    // A buffer outside memory: EFAULT, and nothing goes in.
    iovec(0x80, 1 << 30, 4);
    assert_eq!(errno(vmsplice(w, 1, SpliceFlags(0))), Errno::EFAULT);
    assert_eq!(t.read(r, 0), Ok(Vec::new()));
    assert_eq!(kernel.file(task, r).unwrap().buffered(), 0);
    // Into a full pipe: as much as fits, then EAGAIN.
    let fcntl = block_on(kernel.fcntl64(&t.caller, w, FcntlCmd::F_SETPIPE_SZ, 4096));
    assert_eq!(fcntl, Ok(4096));
    iovec(0x80, 0x1000, 5000);
    assert_eq!(vmsplice(w, 1, SPLICE_F_NONBLOCK), Ok(4096));
    assert_eq!(errno(vmsplice(w, 1, SPLICE_F_NONBLOCK)), Errno::EAGAIN);
}

/// Sockets have `splice_read` and `splice_write`: a stream socket into a pipe and back out.
#[test]
fn splice_between_a_socket_and_a_pipe() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let pair = wasixv2_abi::FdPair::new(0x100);
    let unix = wasixv2_abi::AddressFamily(1);
    const SOCK_STREAM_NONBLOCK: i32 = 1 | 0x800;
    let made = kernel.socketpair(&t.caller, unix, SOCK_STREAM_NONBLOCK, 0, pair);
    assert_eq!(block_on(made), Ok(0));
    let fd = |at| Fd(i32::from_le_bytes(get(&memory, at, 4).try_into().unwrap()));
    let (a, b) = (fd(0x100), fd(0x104));
    let (r, w) = t.pipe(OpenFlags(0));

    assert_eq!(
        errno(t.splice(b, Some(0), w, None, 5, SpliceFlags(0))),
        Errno::EINVAL
    );
    assert_eq!(
        errno(t.splice(b, None, w, None, 5, SpliceFlags(0))),
        Errno::EAGAIN
    );
    assert_eq!(t.write(a, b"ping"), Ok(4));
    assert_eq!(t.splice(b, None, w, None, 100, SpliceFlags(0)), Ok(4));
    assert_eq!(
        t.splice(r, None, b, None, 100, SpliceFlags::SPLICE_F_MORE),
        Ok(4)
    );
    assert_eq!(t.read(a, 16), Ok(b"ping".to_vec()));
}

#[test]
fn file_handles_encode_decode_and_go_stale() {
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    t.create("/d/f", b"data");
    // `struct file_handle` at 0x3000, the mount id at 0x3100.
    let to_handle = |at: &str, room: u32, flags: i32| {
        put(&memory, 0x3000, &room.to_le_bytes());
        let name = t.path(at);
        let handle = FileHandlePtr::new(0x3000);
        let call =
            kernel.name_to_handle_at(&t.caller, CWD, name, handle, S32Ptr::new(0x3100), flags);
        block_on(call)
    };
    let header = || {
        let raw = get(&memory, 0x3000, 8);
        let word = |i: usize| u32::from_le_bytes(raw[i..i + 4].try_into().unwrap());
        (word(0), word(4) as i32)
    };
    let by_handle = |flags: OpenFlags| {
        let open = kernel.open_by_handle_at(&t.caller, CWD, GuestPtr::new(0x3000), flags);
        block_on(open)
    };

    // Too small: the size needed and `FILEID_INVALID`, and the mount id all the same.
    assert_eq!(errno(to_handle("/d/f", 0, 0)), Errno::EOVERFLOW);
    assert_eq!(header(), (12, 255));
    assert_eq!(errno(to_handle("/d/f", 129, 0)), Errno::EINVAL);
    assert_eq!(errno(to_handle("/d/f", 128, 0x4000_0000)), Errno::EINVAL);
    // A connectable handle needs `fh_to_parent`, which tmpfs lacks.
    assert_eq!(errno(to_handle("/d/f", 128, 0x2)), Errno::EOPNOTSUPP);
    assert_eq!(errno(to_handle("/d/f", 128, 0x2 | 0x200)), Errno::EINVAL);

    // tmpfs's own handles (`shmem_encode_fh`), for identifiers too.
    assert_eq!(to_handle("/d/f", 128, 0x200), Ok(0));
    assert_eq!(header(), (12, 1));
    assert_eq!(to_handle("/d/f", 128, 0), Ok(0));
    assert_eq!(header(), (12, 1));
    let fd = by_handle(OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read(Fd(fd), 16), Ok(b"data".to_vec()));
    t.close(Fd(fd));

    // A unique mount id is 64 bits wide.
    assert_eq!(to_handle("/d/f", 128, 0x1), Ok(0));
    let unique = u64::from_le_bytes(get(&memory, 0x3100, 8).try_into().unwrap());
    assert!(unique > 1 << 31);

    // Malformed handles, then an unprivileged caller.
    put(&memory, 0x3000, &0u32.to_le_bytes());
    assert_eq!(errno(by_handle(OpenFlags::O_RDONLY)), Errno::EINVAL);
    put(&memory, 0x3000, &12u32.to_le_bytes());
    put(&memory, 0x3004, &(-1i32).to_le_bytes());
    assert_eq!(errno(by_handle(OpenFlags::O_RDONLY)), Errno::EINVAL);
    put(&memory, 0x3004, &1i32.to_le_bytes());

    // Once the file is gone, its handle is stale.
    let name = t.path("/d/f");
    assert_eq!(
        block_on(kernel.unlinkat(&t.caller, CWD, name, AtFlags(0))),
        Ok(0)
    );
    assert_eq!(errno(by_handle(OpenFlags::O_RDONLY)), Errno::ESTALE);

    // A directory's handle opens the directory, and only CAP_DAC_READ_SEARCH may decode.
    assert_eq!(to_handle("/d", 128, 0), Ok(0));
    let dir = by_handle(OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY).unwrap();
    t.close(Fd(dir));
    assert_eq!(block_on(kernel.setuid(&t.caller, 1000)), Ok(0));
    assert_eq!(errno(by_handle(OpenFlags::O_RDONLY)), Errno::EPERM);
    assert_eq!(
        errno(by_handle(OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY)),
        Errno::EPERM
    );
}

/// fanotify's events read from `fd`: `(mask, fd, pid, info records)` each.
fn fanotify_events(t: &T<'_>, fd: Fd) -> Vec<(u64, i32, i32, Vec<u8>)> {
    block_on(fanotify_read(t, fd))
}

async fn fanotify_read(t: &T<'_>, fd: Fd) -> Vec<(u64, i32, i32, Vec<u8>)> {
    let buf = UserSliceMut::new(0x2000, 4096);
    let Ok(n) = t.kernel.read(&t.caller, fd, buf).await else {
        return Vec::new();
    };
    parse_fanotify(&get(t.memory, 0x2000, n as usize))
}

fn parse_fanotify(raw: &[u8]) -> Vec<(u64, i32, i32, Vec<u8>)> {
    let mut events = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        let len = u32::from_ne_bytes(raw[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(raw[at + 4], 3, "FANOTIFY_METADATA_VERSION");
        let mask = u64::from_ne_bytes(raw[at + 8..at + 16].try_into().unwrap());
        let fd = i32::from_ne_bytes(raw[at + 16..at + 20].try_into().unwrap());
        let pid = i32::from_ne_bytes(raw[at + 20..at + 24].try_into().unwrap());
        events.push((mask, fd, pid, raw[at + 24..at + len].to_vec()));
        at += len;
    }
    events
}

#[test]
fn fanotify_init_checks_flags_and_privilege() {
    use wasixv2_abi::FanotifyInitFlags as F;
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    let init = |flags: F, f_flags: u32| block_on(kernel.fanotify_init(&t.caller, flags.0, f_flags));

    assert_eq!(errno(init(F(0x8000_0000), 0)), Errno::EINVAL);
    assert_eq!(errno(init(F::FAN_CLASS_NOTIF, 3)), Errno::EINVAL);
    assert_eq!(
        errno(init(F::FAN_CLASS_CONTENT | F::FAN_REPORT_FID, 0)),
        Errno::EINVAL
    );
    assert_eq!(errno(init(F::FAN_REPORT_NAME, 0)), Errno::EINVAL);
    assert_eq!(
        errno(init(F::FAN_REPORT_TARGET_FID | F::FAN_REPORT_DFID_NAME, 0)),
        Errno::EINVAL
    );
    assert!(init(F::FAN_REPORT_MNT, 0).is_ok());
    assert!(init(F::FAN_CLASS_CONTENT, 0).is_ok());

    // Without CAP_SYS_ADMIN, only identifier groups, and no marks on mounts.
    assert_eq!(block_on(kernel.setuid(&t.caller, 1000)), Ok(0));
    assert_eq!(errno(init(F::FAN_CLASS_NOTIF, 0)), Errno::EPERM);
    assert_eq!(
        errno(init(F::FAN_REPORT_FID | F::FAN_UNLIMITED_QUEUE, 0)),
        Errno::EPERM
    );
    let fd = Fd(init(F::FAN_REPORT_FID, 0).unwrap());
    let name = t.path("/");
    let mark = kernel.fanotify_mark(&t.caller, fd, 0x11, 0x2, CWD, name);
    assert_eq!(errno(block_on(mark)), Errno::EPERM);
}

#[test]
fn fanotify_reports_opens_writes_and_closes_with_a_descriptor() {
    use wasixv2_abi::{FanotifyMarkFlags as M, FanotifyMask as E};
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    t.create("/d/f", b"x");
    let group = Fd(block_on(kernel.fanotify_init(&t.caller, 0x2, 0)).unwrap());
    let mark = |flags: M, mask: E, at: &str| {
        let name = t.path(at);
        block_on(kernel.fanotify_mark(&t.caller, group, flags.0, mask.0, CWD, name))
    };
    let wanted = E::FAN_OPEN | E::FAN_MODIFY | E::FAN_CLOSE_WRITE;
    // Inode events need identifiers; a mask of nothing is EINVAL; removing nothing is ENOENT.
    assert_eq!(
        errno(mark(M::FAN_MARK_ADD, E::FAN_CREATE, "/d")),
        Errno::EINVAL
    );
    assert_eq!(errno(mark(M::FAN_MARK_ADD, E(0), "/d/f")), Errno::EINVAL);
    assert_eq!(
        errno(mark(M::FAN_MARK_REMOVE, wanted, "/d/f")),
        Errno::ENOENT
    );
    assert_eq!(mark(M::FAN_MARK_ADD, wanted, "/d/f"), Ok(0));

    let me = block_on(kernel.getpid(&t.caller)).unwrap();
    let name = t.path("/d/f");
    put(&memory, 0x2000, b"y");
    block_on(wasixv2_syscalls::as_task(task, async {
        let open = kernel.openat(&t.caller, CWD, name, OpenFlags::O_WRONLY, 0);
        let fd = open.await.unwrap();
        let write = kernel.write(&t.caller, fd, UserSlice::new(0x2000, 1));
        assert_eq!(write.await, Ok(1));
        assert_eq!(kernel.close(&t.caller, fd).await, Ok(0));
    }));
    // One event: same file, same process, merged (`fanotify_merge`).
    let events = fanotify_events(&t, group);
    assert_eq!(events.len(), 1);
    let (mask, fd, pid, info) = &events[0];
    assert_eq!(*mask, wanted.0);
    assert_eq!(*pid, me);
    assert!(info.is_empty());
    // The descriptor reads the file, and makes no events of its own.
    assert_eq!(t.read(Fd(*fd), 8), Ok(b"y".to_vec()));
    t.close(Fd(*fd));
    assert_eq!(fanotify_events(&t, group), []);

    // A mount mark hears of every file on the mount; the file's own mark of its opens.
    assert_eq!(
        mark(M::FAN_MARK_MOUNT | M::FAN_MARK_ADD, E::FAN_ACCESS, "/"),
        Ok(0)
    );
    let fd = t.open("/d/f", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(t.read(fd, 8), Ok(b"y".to_vec()));
    let events = fanotify_events(&t, group);
    let masks: Vec<u64> = events.iter().map(|e| e.0).collect();
    assert_eq!(masks, [(E::FAN_OPEN | E::FAN_ACCESS).0]);
    t.close(Fd(events[0].1));
    t.close(fd);
    let flush = M::FAN_MARK_MOUNT | M::FAN_MARK_FLUSH;
    let name = t.path("/");
    let call = kernel.fanotify_mark(&t.caller, group, flush.0, 0, CWD, name);
    assert_eq!(block_on(call), Ok(0));
}

/// Linux fs/notify/fanotify/fanotify.c `fanotify_alloc_event` records current's pid or tgid.
/// The syscall caller can differ from the task whose instance originally started running.
#[test]
fn fanotify_syscall_entry_reports_the_callers_pid_and_tid() {
    use wasixv2_abi::{FanotifyInitFlags as F, FanotifyMarkFlags as M, FanotifyMask as E};
    use wasixv2_syscalls::Layered;

    for report_tid in [false, true] {
        let (kernel, task, memory) = super::cred::machine_as(0, 0);
        let t = T::new(&kernel, task, &memory);
        t.create("/f", b"x");
        let flags = F::FAN_NONBLOCK | if report_tid { F::FAN_REPORT_TID } else { F(0) };
        let group = Fd(block_on(kernel.fanotify_init(&t.caller, flags.0, 0)).unwrap());
        let name = t.path("/f");
        let mark = kernel.fanotify_mark(
            &t.caller,
            group,
            M::FAN_MARK_ADD.0,
            E::FAN_OPEN.0,
            CWD,
            name,
        );
        assert_eq!(block_on(mark), Ok(0));
        let spawn = kernel.thread_spawn(
            &t.caller,
            0x10f00,
            0,
            GuestPtrMut::NULL,
            0,
            GuestPtrMut::NULL,
        );
        let tid = block_on(spawn).unwrap();
        while kernel.take_pending(task).is_some() {}
        let caller = Caller::new(TaskId(tid as u32), &memory);
        let expected = if report_tid {
            tid
        } else {
            block_on(kernel.getpid(&t.caller)).unwrap()
        };
        let stack = Layered::new(SyscallEntry(Dispatch), kernel);
        let t = T::new(&stack.inner, task, &memory);
        for outer in [None, Some(task)] {
            // Each call works without an executor scope, or overrides and restores it.
            let run = async {
                let fd = stack
                    .openat(&caller, CWD, name, OpenFlags::O_RDONLY, 0)
                    .await
                    .unwrap();
                assert_eq!(wasixv2_syscalls::current_task(), outer);
                stack.close(&caller, fd).await.unwrap();
            };
            match outer {
                None => block_on(run),
                Some(task) => block_on(wasixv2_syscalls::as_task(task, run)),
            }
            let events = fanotify_events(&t, group);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].2, expected);
            t.close(Fd(events[0].1));
        }
        assert_eq!(wasixv2_syscalls::current_task(), None);
    }
}

#[test]
fn fanotify_reports_directory_entries_by_handle_and_name() {
    use wasixv2_abi::{FanotifyInitFlags as F, FanotifyMarkFlags as M, FanotifyMask as E};
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    let flags = F::FAN_REPORT_DFID_NAME | F::FAN_NONBLOCK;
    let group = Fd(block_on(kernel.fanotify_init(&t.caller, flags.0, 0)).unwrap());
    let mask = E::FAN_CREATE | E::FAN_DELETE | E::FAN_ONDIR;
    let name = t.path("/d");
    let add = kernel.fanotify_mark(&t.caller, group, M::FAN_MARK_ADD.0, mask.0, CWD, name);
    assert_eq!(block_on(add), Ok(0));
    // The directory's handle, to compare with what the events report.
    put(&memory, 0x3000, &128u32.to_le_bytes());
    let name = t.path("/d");
    let handle = FileHandlePtr::new(0x3000);
    let call = kernel.name_to_handle_at(&t.caller, CWD, name, handle, S32Ptr::new(0x3100), 0);
    assert_eq!(block_on(call), Ok(0));
    let dir_handle = get(&memory, 0x3000, 8 + 12);

    t.create("/d/x", b"");
    t.mkdir("/d/sub");
    let events = fanotify_events(&t, group);
    let masks: Vec<u64> = events.iter().map(|e| e.0).collect();
    assert_eq!(masks, [E::FAN_CREATE.0, (E::FAN_CREATE | E::FAN_ONDIR).0]);
    // One `FAN_EVENT_INFO_TYPE_DFID_NAME` record: header, fsid, the handle, "x\0", padded.
    let (_, fd, _, info) = &events[0];
    assert_eq!(*fd, -1);
    assert_eq!(info[0], 2);
    assert_eq!(u16::from_ne_bytes([info[2], info[3]]) as usize, info.len());
    assert_eq!(info.len(), (12 + 8 + 12 + 2usize).next_multiple_of(4));
    assert_eq!(&info[12..32], &dir_handle[..]);
    assert_eq!(&info[32..34], b"x\0");
    assert_eq!(&events[1].3[32..36], b"sub\0");
}

#[test]
fn fanotify_permission_events_wait_for_an_answer() {
    use futures::future::join;
    use wasixv2_abi::{FanotifyInitFlags as F, FanotifyMarkFlags as M, FanotifyMask as E};
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.create("/f", b"x");
    let flags = F::FAN_CLASS_CONTENT | F::FAN_NONBLOCK;
    let group = Fd(block_on(kernel.fanotify_init(&t.caller, flags.0, 0)).unwrap());
    let name = t.path("/f");
    let add = kernel.fanotify_mark(
        &t.caller,
        group,
        M::FAN_MARK_ADD.0,
        E::FAN_OPEN_PERM.0,
        CWD,
        name,
    );
    assert_eq!(block_on(add), Ok(0));
    let respond_async = |fd: i32, response: u32| {
        let mut raw = fd.to_ne_bytes().to_vec();
        raw.extend_from_slice(&response.to_ne_bytes());
        put(&memory, 0x2800, &raw);
        kernel.write(&t.caller, group, UserSlice::new(0x2800, 8))
    };
    let respond = |fd: i32, response: u32| block_on(respond_async(fd, response));
    // Answers are checked before they are looked up.
    assert_eq!(errno(respond(3, 0x4)), Errno::EINVAL);
    assert_eq!(errno(respond(3, 0x1 | 0x10)), Errno::EINVAL);
    assert_eq!(errno(respond(77, 0x1)), Errno::ENOENT);

    // Denied: the open fails with EPERM once the listener says so.
    let name = t.path("/f");
    let opener = wasixv2_syscalls::as_task(task, async {
        kernel
            .openat(&t.caller, CWD, name, OpenFlags::O_RDONLY, 0)
            .await
    });
    let listener = async {
        let events = fanotify_read(&t, group).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, E::FAN_OPEN_PERM.0);
        let fd = events[0].1;
        assert_eq!(respond_async(fd, 0x2).await, Ok(8));
        assert_eq!(errno(respond_async(fd, 0x2).await), Errno::ENOENT);
        assert_eq!(kernel.close(&t.caller, Fd(fd)).await, Ok(0));
    };
    let (opened, ()) = block_on(join(opener, listener));
    assert_eq!(errno(opened), Errno::EPERM);

    // Allowed, and closing the group lets everything waiting through.
    let name = t.path("/f");
    let opener = kernel.openat(&t.caller, CWD, name, OpenFlags::O_RDONLY, 0);
    let listener = async {
        let events = fanotify_read(&t, group).await;
        assert_eq!(events.len(), 1);
        assert_eq!(kernel.close(&t.caller, Fd(events[0].1)).await, Ok(0));
        assert_eq!(kernel.close(&t.caller, group).await, Ok(0));
    };
    let (opened, ()) = block_on(join(opener, listener));
    assert!(opened.is_ok());
}

#[test]
fn inode_flags_make_files_append_only_or_immutable() {
    use wasixv2_abi::{FsInodeFlags as Fl, IoctlRequest as R, StatxMask};
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    t.create("/d/f", b"data");
    let at = |addr: u32, path: &str| {
        let mut bytes = path.as_bytes().to_vec();
        bytes.push(0);
        put(&memory, addr, &bytes);
        Cstr::new(addr)
    };
    let fd = t.open("/d/f", OpenFlags::O_RDONLY).unwrap();
    let get_flags = |fd: Fd| {
        let call = kernel.ioctl(&t.caller, fd, R::FS_IOC_GETFLAGS, 0x3000);
        block_on(call).map(|_| u32::from_le_bytes(get(&memory, 0x3000, 4).try_into().unwrap()))
    };
    let set_flags = |fd: Fd, flags: u32| {
        put(&memory, 0x3000, &flags.to_le_bytes());
        block_on(kernel.ioctl(&t.caller, fd, R::FS_IOC_SETFLAGS, 0x3000))
    };
    assert_eq!(get_flags(fd), Ok(0));
    // tmpfs keeps only immutable, append-only, nodump and noatime.
    assert_eq!(errno(set_flags(fd, Fl::FS_COMPR_FL.0)), Errno::EOPNOTSUPP);

    // Append-only: written only by appending, never truncated, removed or renamed.
    assert_eq!(set_flags(fd, Fl::FS_APPEND_FL.0), Ok(0));
    assert_eq!(get_flags(fd), Ok(Fl::FS_APPEND_FL.0));
    assert_eq!(errno(t.open("/d/f", OpenFlags::O_WRONLY)), Errno::EPERM);
    let trunc = OpenFlags::O_WRONLY | OpenFlags::O_APPEND | OpenFlags::O_TRUNC;
    assert_eq!(errno(t.open("/d/f", trunc)), Errno::EPERM);
    let appender = t
        .open("/d/f", OpenFlags::O_WRONLY | OpenFlags::O_APPEND)
        .unwrap();
    assert_eq!(t.write(appender, b"+"), Ok(1));
    let setfl = kernel.fcntl64(&t.caller, appender, FcntlCmd::F_SETFL, 0);
    assert_eq!(errno(block_on(setfl)), Errno::EPERM);
    assert_eq!(
        errno(block_on(kernel.ftruncate64(&t.caller, appender, 0))),
        Errno::EPERM
    );
    assert_eq!(
        errno(block_on(kernel.truncate64(
            &t.caller,
            at(0x1000, "/d/f"),
            0
        ))),
        Errno::EPERM
    );
    let unlink = kernel.unlinkat(&t.caller, CWD, at(0x1000, "/d/f"), AtFlags(0));
    assert_eq!(errno(block_on(unlink)), Errno::EPERM);
    // `statx` shows it.
    let statx = kernel.statx(
        &t.caller,
        CWD,
        at(0x1000, "/d/f"),
        AtFlags(0),
        StatxMask::STATX_BASIC_STATS,
        GuestPtrMut::new(0x4000),
    );
    assert_eq!(block_on(statx), Ok(0));
    let attributes = u64::from_le_bytes(get(&memory, 0x4008, 8).try_into().unwrap());
    assert_eq!(attributes & 0x30, 0x20);
    t.close(appender);

    // Immutable: readable, nothing else.
    assert_eq!(set_flags(fd, Fl::FS_IMMUTABLE_FL.0), Ok(0));
    assert!(t.open("/d/f", OpenFlags::O_RDONLY).is_ok());
    let append = OpenFlags::O_WRONLY | OpenFlags::O_APPEND;
    assert_eq!(errno(t.open("/d/f", append)), Errno::EPERM);
    let chmod = kernel.fchmodat(&t.caller, CWD, at(0x1000, "/d/f"), 0o600);
    assert_eq!(errno(block_on(chmod)), Errno::EPERM);
    let link = kernel.linkat(
        &t.caller,
        CWD,
        at(0x1000, "/d/f"),
        CWD,
        at(0x1100, "/d/g"),
        AtFlags(0),
    );
    assert_eq!(errno(block_on(link)), Errno::EPERM);
    let rename = kernel.renameat2(
        &t.caller,
        CWD,
        at(0x1000, "/d/f"),
        CWD,
        at(0x1100, "/d/g"),
        wasixv2_abi::RenameFlags(0),
    );
    assert_eq!(errno(block_on(rename)), Errno::EPERM);
    let touch = kernel.utimensat_time64(
        &t.caller,
        CWD,
        at(0x1000, "/d/f"),
        GuestPtr::new(0),
        AtFlags(0),
    );
    assert_eq!(errno(block_on(touch)), Errno::EPERM);

    // Only CAP_LINUX_IMMUTABLE changes those two; the owner sets the others.
    assert_eq!(set_flags(fd, 0), Ok(0));
    let name = t.path("/d/f");
    assert_eq!(
        block_on(kernel.fchownat(&t.caller, CWD, name, 1000, 1000, AtFlags(0))),
        Ok(0)
    );
    assert_eq!(block_on(kernel.setuid(&t.caller, 1000)), Ok(0));
    assert_eq!(errno(set_flags(fd, Fl::FS_APPEND_FL.0)), Errno::EPERM);
    assert_eq!(set_flags(fd, Fl::FS_NODUMP_FL.0), Ok(0));
    assert_eq!(get_flags(fd), Ok(Fl::FS_NODUMP_FL.0));
}

#[test]
fn file_attrs_through_fsxattr_and_file_getattr() {
    use wasixv2_abi::{FsInodeFlags as Fl, FsXflags as X, IoctlRequest as R};
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/d");
    let dir = t.open("/d", OpenFlags::O_RDONLY).unwrap();
    // FS_IOC_FSSETXATTR with a flag tmpfs keeps, then with an extent size it does not.
    let fsxattr = |xflags: u32, extsize: u32| {
        let mut raw = [0u8; 28];
        raw[..4].copy_from_slice(&xflags.to_le_bytes());
        raw[4..8].copy_from_slice(&extsize.to_le_bytes());
        put(&memory, 0x3000, &raw);
        block_on(kernel.ioctl(&t.caller, dir, R::FS_IOC_FSSETXATTR, 0x3000))
    };
    assert_eq!(fsxattr(X::FS_XFLAG_NODUMP.0, 0), Ok(0));
    assert_eq!(
        errno(fsxattr(X::FS_XFLAG_NODUMP.0, 4096)),
        Errno::EOPNOTSUPP
    );
    assert_eq!(errno(fsxattr(0x4000_0000, 0)), Errno::EOPNOTSUPP);
    assert_eq!(
        block_on(kernel.ioctl(&t.caller, dir, R::FS_IOC_FSGETXATTR, 0x3000)),
        Ok(0)
    );
    assert_eq!(get(&memory, 0x3000, 4), X::FS_XFLAG_NODUMP.0.to_le_bytes());

    // New objects inherit nodump; file_getattr reports it as an extended flag.
    t.create("/d/x", b"");
    let name = t.path("/d/x");
    let getattr = |size: u32| {
        let call = kernel.file_getattr(
            &t.caller,
            CWD,
            name,
            GuestPtrMut::new(0x3000),
            size,
            AtFlags(0),
        );
        block_on(call)
    };
    assert_eq!(errno(getattr(16)), Errno::EINVAL);
    assert_eq!(errno(getattr(8192)), Errno::E2BIG);
    put(&memory, 0x3000, &[0xff; 32]);
    assert_eq!(getattr(32), Ok(0));
    let raw = get(&memory, 0x3000, 32);
    assert_eq!(
        u64::from_le_bytes(raw[..8].try_into().unwrap()),
        u64::from(X::FS_XFLAG_NODUMP.0)
    );
    assert!(raw[24..].iter().all(|&b| b == 0));

    // file_setattr: append-only through the extended flags, which FS_IOC_GETFLAGS then shows.
    let mut attr = [0u8; 24];
    attr[..8].copy_from_slice(&u64::from(X::FS_XFLAG_APPEND.0).to_le_bytes());
    put(&memory, 0x3400, &attr);
    let setattr = kernel.file_setattr(&t.caller, CWD, name, GuestPtr::new(0x3400), 24, AtFlags(0));
    assert_eq!(block_on(setattr), Ok(0));
    let fd = t.open("/d/x", OpenFlags::O_RDONLY).unwrap();
    assert_eq!(
        block_on(kernel.ioctl(&t.caller, fd, R::FS_IOC_GETFLAGS, 0x3000)),
        Ok(0)
    );
    assert_eq!(get(&memory, 0x3000, 4), Fl::FS_APPEND_FL.0.to_le_bytes());
}

/// A POSIX ACL value: `(tag, perm, id)` entries after the version 2 header.
fn acl_value(entries: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut out = 2u32.to_le_bytes().to_vec();
    for &(tag, perm, id) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&perm.to_le_bytes());
        out.extend_from_slice(&id.to_le_bytes());
    }
    out
}

#[test]
fn posix_acls_grant_inherit_and_follow_chmod() {
    use wasixv2_abi::{StatxMask, XattrFlags};
    const U: u32 = u32::MAX;
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    let at = |addr: u32, s: &str| {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        put(&memory, addr, &bytes);
        Cstr::new(addr)
    };
    let set = |path: &str, name: &str, value: &[u8]| {
        put(&memory, 0x3000, value);
        let call = kernel.setxattr(
            &t.caller,
            at(0x1000, path),
            at(0x1200, name),
            UserSlice::new(0x3000, value.len() as u32),
            XattrFlags(0),
        );
        block_on(call)
    };
    let get = |path: &str, name: &str| {
        let call = kernel.getxattr(
            &t.caller,
            at(0x1000, path),
            at(0x1200, name),
            UserSliceMut::new(0x3800, 256),
        );
        block_on(call).map(|n| super::ipc::get(&memory, 0x3800, n as usize))
    };
    let mode = |path: &str| {
        let call = kernel.statx(
            &t.caller,
            CWD,
            at(0x1000, path),
            AtFlags(0),
            StatxMask::STATX_BASIC_STATS,
            GuestPtrMut::new(0x4000),
        );
        assert_eq!(block_on(call), Ok(0));
        u16::from_le_bytes(get_bytes(&memory, 0x4000 + 28)) & 0o7777
    };
    fn get_bytes(memory: &FlatMemory, addr: u32) -> [u8; 2] {
        super::ipc::get(memory, addr, 2).try_into().unwrap()
    }
    let access = "system.posix_acl_access";
    let default = "system.posix_acl_default";

    t.create("/f", b"secret");
    assert_eq!(
        block_on(kernel.fchmodat(&t.caller, CWD, at(0x1000, "/f"), 0o600)),
        Ok(0)
    );
    // u::rw-, u:2000:rw-, g::r--, m::rw-, o::---: the mask becomes the group bits.
    let grant = acl_value(&[
        (1, 6, U),
        (2, 6, 2000),
        (4, 4, U),
        (0x10, 6, U),
        (0x20, 0, U),
    ]);
    assert_eq!(set("/f", access, &grant), Ok(0));
    assert_eq!(mode("/f"), 0o660);
    assert_eq!(get("/f", access), Ok(grant.clone()));
    // Malformed or invalid: a named user needs a mask; only a directory has a default ACL.
    let no_mask = acl_value(&[(1, 6, U), (2, 6, 2000), (4, 4, U), (0x20, 0, U)]);
    assert_eq!(errno(set("/f", access, &no_mask)), Errno::EINVAL);
    assert_eq!(errno(set("/f", access, &[3, 0, 0, 0])), Errno::EOPNOTSUPP);
    assert_eq!(errno(set("/f", default, &grant)), Errno::EACCES);

    // An ACL a mode can say is that mode, and no ACL.
    t.create("/plain", b"");
    let equiv = acl_value(&[(1, 7, U), (4, 5, U), (0x20, 5, U)]);
    assert_eq!(set("/plain", access, &equiv), Ok(0));
    assert_eq!(mode("/plain"), 0o755);
    assert_eq!(errno(get("/plain", access)), Errno::ENODATA);

    // A directory's default ACL is what is made in it gets, the umask aside.
    t.mkdir("/d");
    let inherit = acl_value(&[
        (1, 7, U),
        (2, 7, 2000),
        (4, 5, U),
        (0x10, 7, U),
        (0x20, 0, U),
    ]);
    assert_eq!(set("/d", default, &inherit), Ok(0));
    // Made with 0644 under a 022 umask: the ACL limits it to 0640, and itself to the mode.
    t.create("/d/x", b"");
    assert_eq!(mode("/d/x"), 0o640);
    let made = acl_value(&[
        (1, 6, U),
        (2, 7, 2000),
        (4, 5, U),
        (0x10, 4, U),
        (0x20, 0, U),
    ]);
    assert_eq!(get("/d/x", access), Ok(made));
    t.mkdir("/d/sub");
    assert_eq!(get("/d/sub", default), Ok(inherit.clone()));

    // chmod rewrites the mask, so the named user loses what it was given.
    t.create("/g", b"");
    assert_eq!(set("/g", access, &grant), Ok(0));
    assert_eq!(
        block_on(kernel.fchmodat(&t.caller, CWD, at(0x1000, "/g"), 0o600)),
        Ok(0)
    );
    let masked = acl_value(&[
        (1, 6, U),
        (2, 6, 2000),
        (4, 4, U),
        (0x10, 0, U),
        (0x20, 0, U),
    ]);
    assert_eq!(get("/g", access), Ok(masked));

    // User 2000, neither owner nor in the group, reads and writes /f through its entry.
    assert_eq!(block_on(kernel.setuid(&t.caller, 2000)), Ok(0));
    assert!(t.open("/f", OpenFlags::O_RDWR).is_ok());
    assert_eq!(errno(t.open("/g", OpenFlags::O_RDONLY)), Errno::EACCES);
    assert_eq!(errno(set("/f", access, &grant)), Errno::EPERM);
}

#[test]
fn tmpfs_quotactl_fd_controls_limits_and_checks_permissions() {
    use wasixv2_abi::{IfDqblk, IfDqinfo, QuotaMisc as Q};
    use wasixv2_vfs::{Cap, MountFlags};
    use wasixv2_vfs_mem::{Limits, QuotaLimits};
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/quota");
    let fs = Arc::new(
        MemFs::with_limits_and_quotas(
            Arc::new(HostClock(Arc::new(TestHost))),
            0o777,
            0,
            0,
            Limits::UNLIMITED,
            QuotaLimits {
                types: 1,
                ..QuotaLimits::default()
            },
        )
        .unwrap(),
    );
    block_on(
        kernel
            .vfs
            .mount(b"/quota", fs.clone(), MountFlags::default(), false),
    )
    .unwrap();
    let dir = t.open("/quota", OpenFlags::O_PATH).unwrap();
    let quota = |fd, op: Q, kind: u32, id| {
        block_on(kernel.quotactl_fd(
            &t.caller,
            fd,
            op.0 << 8 | kind,
            id,
            GuestPtrMut::new(0x4000),
        ))
    };
    assert_eq!(errno(quota(Fd(-1), Q::Q_GETFMT, 3, 0)), Errno::EBADF);
    assert_eq!(errno(quota(dir, Q::Q_GETFMT, 3, 0)), Errno::EINVAL);
    assert_eq!(errno(quota(dir, Q::Q_GETFMT, 2, 0)), Errno::EINVAL);
    assert_eq!(errno(quota(dir, Q::Q_GETFMT, 1, 0)), Errno::ESRCH);
    assert_eq!(quota(dir, Q::Q_GETFMT, 0, 0), Ok(0));
    assert_eq!(
        u32::from_le_bytes(get(&memory, 0x4000, 4).try_into().unwrap()),
        Q::QFMT_SHMEM.0
    );
    assert_eq!(quota(dir, Q::Q_GETINFO, 0, 0), Ok(0));
    let info: IfDqinfo =
        block_on(wasixv2_syscalls::read_value(&memory, GuestPtr::new(0x4000))).unwrap();
    assert_eq!(
        (info.dqi_bgrace, info.dqi_igrace, info.dqi_flags),
        (604800, 604800, 1 << 16)
    );
    let file = t
        .open("/quota/f", OpenFlags::O_CREAT | OpenFlags::O_RDWR)
        .unwrap();
    let limits = IfDqblk {
        dqb_bhardlimit: 4,
        dqb_valid: Q::QIF_BLIMITS.0,
        ..IfDqblk::default()
    };
    block_on(wasixv2_syscalls::write_value(
        &memory,
        GuestPtrMut::new(0x4000),
        &limits,
    ))
    .unwrap();
    assert_eq!(quota(dir, Q::Q_SETQUOTA, 0, 0), Ok(0));
    // Dropping only CAP_SYS_RESOURCE makes uid 0 obey the limits, even on an existing fd.
    super::cred::set_caps(
        &kernel,
        &t.caller,
        &memory,
        crate::cred::CAP_FULL & !Cap::SysResource.bit(),
    );
    assert_eq!(t.write(file, &[0; 8192]), Ok(4096));
    assert_eq!(errno(t.write(file, b"over")), Errno::EDQUOT);
    assert_eq!(quota(dir, Q::Q_GETQUOTA, 0, 0), Ok(0));
    let dq: IfDqblk =
        block_on(wasixv2_syscalls::read_value(&memory, GuestPtr::new(0x4000))).unwrap();
    assert_eq!((dq.dqb_curspace, dq.dqb_curinodes), (4096, 2));
    assert_eq!(quota(dir, Q::Q_QUOTAON, 0, 0), Err(Errno::EEXIST.into()));
    assert_eq!(quota(dir, Q::Q_QUOTAOFF, 0, 0), Ok(0));
    assert_eq!(t.write(file, b"accounted"), Ok(9));
    assert_eq!(quota(dir, Q::Q_QUOTAON, 0, 0), Ok(0));
    assert_eq!(quota(dir, Q::Q_GETNEXTQUOTA, 0, 0), Ok(0));
    assert_eq!(block_on(kernel.setuid(&t.caller, 1)), Ok(0));
    assert_eq!(errno(quota(dir, Q::Q_GETQUOTA, 0, 0)), Errno::EPERM);
    assert_eq!(quota(dir, Q::Q_GETQUOTA, 0, 1), Ok(0));
    assert_eq!(errno(quota(dir, Q::Q_GETNEXTQUOTA, 0, 0)), Errno::EPERM);
    assert_eq!(errno(quota(dir, Q::Q_SETQUOTA, 0, 1)), Errno::EPERM);
    assert_eq!(quota(dir, Q::Q_GETFMT, 0, 0), Ok(0));
    assert_eq!(
        block_on(kernel.quotactl(
            &t.caller,
            Q::Q_SYNC.0 << 8,
            GuestPtr::new(0),
            0,
            GuestPtrMut::new(0)
        )),
        Ok(0)
    );
    assert_eq!(
        errno(block_on(kernel.quotactl(
            &t.caller,
            Q::Q_GETQUOTA.0 << 8,
            t.path("/quota"),
            1,
            GuestPtrMut::new(0)
        ))),
        Errno::ENOTBLK
    );
}

#[test]
fn fanotify_pre_access_reports_aligned_ranges_and_mediates_io_and_mmap() {
    use futures::future::join;
    use wasixv2_abi::{
        FanotifyInitFlags as F, FanotifyMarkFlags as M, FanotifyMask as E, MapFlags, ProtFlags,
    };
    use wasixv2_vfs::{FsResult, MountFlags, Node, StatFs};
    struct HsmFs(MemFs);
    #[async_trait::async_trait(?Send)]
    impl Filesystem for HsmFs {
        fn info(&self) -> wasixv2_vfs::FsInfo {
            self.0.info()
        }
        fn root(&self) -> Arc<dyn Node> {
            self.0.root()
        }
        fn allow_hsm(&self) -> bool {
            true
        }
        async fn statfs(&self) -> FsResult<StatFs> {
            self.0.statfs().await
        }
    }
    let (kernel, task, memory) = super::cred::machine_as(0, 0);
    let t = T::new(&kernel, task, &memory);
    t.mkdir("/hsm");
    let fs = Arc::new(HsmFs(MemFs::new(
        Arc::new(HostClock(Arc::new(TestHost))),
        0o777,
        0,
        0,
    )));
    block_on(kernel.vfs.mount(b"/hsm", fs, MountFlags::default(), false)).unwrap();
    t.create("/hsm/f", b"old");
    let file = t.open("/hsm/f", OpenFlags::O_RDWR).unwrap();
    let group = Fd(block_on(kernel.fanotify_init(
        &t.caller,
        (F::FAN_CLASS_PRE_CONTENT | F::FAN_NONBLOCK).0,
        0,
    ))
    .unwrap());
    let name = t.path("/hsm/f");
    assert_eq!(
        block_on(kernel.fanotify_mark(
            &t.caller,
            group,
            M::FAN_MARK_ADD.0,
            E::FAN_PRE_ACCESS.0,
            CWD,
            name
        )),
        Ok(0)
    );
    async fn answer(t: &T<'_>, group: Fd, offset: u64, count: u64, response: u32) {
        let events = fanotify_read(t, group).await;
        assert_eq!(events.len(), 1);
        let (mask, fd, _, info) = &events[0];
        assert_eq!(*mask, wasixv2_abi::FanotifyMask::FAN_PRE_ACCESS.0);
        assert_eq!(info.len(), 24);
        assert_eq!(info[0], 6); // FAN_EVENT_INFO_TYPE_RANGE
        assert_eq!(u64::from_ne_bytes(info[8..16].try_into().unwrap()), offset);
        assert_eq!(u64::from_ne_bytes(info[16..24].try_into().unwrap()), count);
        let raw = [fd.to_ne_bytes(), response.to_ne_bytes()].concat();
        put(t.memory, 0x2800, &raw);
        assert_eq!(
            t.kernel
                .write(&t.caller, group, UserSlice::new(0x2800, 8))
                .await,
            Ok(8)
        );
        assert_eq!(t.kernel.close(&t.caller, Fd(*fd)).await, Ok(0));
    }
    put(&memory, 0x2000, b"new");
    let write = || kernel.pwrite64(&t.caller, file, UserSlice::new(0x2000, 3), 0);
    let (written, ()) = block_on(join(write(), answer(&t, group, 0, WASM_PAGE_SIZE, 2)));
    assert_eq!(errno(written), Errno::EPERM);
    let (written, ()) = block_on(join(write(), answer(&t, group, 0, WASM_PAGE_SIZE, 1)));
    assert_eq!(written, Ok(3));
    let read = kernel.pread64(
        &t.caller,
        file,
        UserSliceMut::new(0x2000, 2),
        WASM_PAGE_SIZE as i64 - 1,
    );
    let (read, ()) = block_on(join(read, answer(&t, group, 0, 2 * WASM_PAGE_SIZE, 1)));
    assert_eq!(read, Ok(0));
    let trunc = kernel.ftruncate64(&t.caller, file, 123);
    let (truncated, ()) = block_on(join(trunc, answer(&t, group, 0, WASM_PAGE_SIZE, 1)));
    assert_eq!(truncated, Ok(0));
    let mmap = kernel.mmap(
        &t.caller,
        0,
        WASM_PAGE_SIZE as u32,
        ProtFlags::PROT_READ,
        MapFlags::MAP_PRIVATE,
        file,
        0,
    );
    let (mapped, ()) = block_on(join(mmap, answer(&t, group, 0, WASM_PAGE_SIZE, 2)));
    assert_eq!(errno(mapped), Errno::EPERM);
    // tmpfs remains opted out, matching Linux's absence of SB_I_ALLOW_HSM there.
    t.create("/plain", b"");
    let name = t.path("/plain");
    assert_eq!(
        errno(block_on(kernel.fanotify_mark(
            &t.caller,
            group,
            M::FAN_MARK_ADD.0,
            E::FAN_PRE_ACCESS.0,
            CWD,
            name
        ))),
        Errno::EOPNOTSUPP
    );
}
