//! FUSE tests use a daemon driving the guest device descriptor, with no host FUSE dependency.
use super::{
    cred::machine_as,
    ipc::{get, put},
    *,
};
use std::future::Future;
use wasixv2_abi::{IoctlRequest, MountFlags as MsFlags};
use wasixv2_syscalls::as_task;
use wasixv2_vfs::{
    AllocMode, FileType, NewKind, NewNode, Node, OpenOptions, RenameMode, SeekHole, SetAttr,
    XattrSet,
};
const CWD: Fd = Fd(-100);
const BUF: u32 = 0x10000;

fn n32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn n64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn p32(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn p64(b: &mut [u8], at: usize, n: u64) {
    b[at..at + 8].copy_from_slice(&n.to_le_bytes());
}
fn attr(id: u64, kind: FileType) -> Vec<u8> {
    let mut b = vec![0; 88];
    p64(&mut b, 0, id);
    p64(&mut b, 8, 64);
    p32(&mut b, 60, kind.mode_bits() | 0o777);
    p32(&mut b, 64, 1);
    p32(&mut b, 80, 4096);
    b
}
fn entry(id: u64, kind: FileType) -> Vec<u8> {
    let mut b = vec![0; 40];
    p64(&mut b, 0, id);
    p64(&mut b, 8, 1);
    p64(&mut b, 16, 100);
    p64(&mut b, 24, 100);
    b.extend(attr(id, kind));
    b
}
fn attr_out(id: u64, kind: FileType) -> Vec<u8> {
    let mut b = vec![0; 16];
    p64(&mut b, 0, 100);
    b.extend(attr(id, kind));
    b
}
fn now<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("nonblocking daemon operation slept"),
    }
}
struct Daemon {
    kernel: Kernel,
    task: TaskId,
    memory: FlatMemory,
    fd: Fd,
    root: Arc<dyn Node>,
}
impl Daemon {
    fn new() -> Self {
        Self::with_mount("fuse", "")
    }
    fn with_mount(fstype: &str, extra: &str) -> Self {
        let (kernel, task, mut memory) = machine_as(0, 0);
        memory.0.get_mut().resize(256 * 1024, 0);
        let caller = Caller::new(task, &memory);
        for path in [b"/dev\0".as_slice(), b"/mnt\0".as_slice()] {
            put(&memory, 0x100, path);
            block_on(kernel.mkdirat(&caller, CWD, GuestPtr::new(0x100), 0o755)).unwrap();
        }
        block_on(kernel.create_device_nodes(b"/dev")).unwrap();
        put(&memory, 0x100, b"/dev/fuse\0");
        let fd = block_on(kernel.openat(
            &caller,
            CWD,
            GuestPtr::new(0x100),
            OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK,
            0,
        ))
        .unwrap();
        put(&memory, 0x200, b"/mnt\0");
        put(&memory, 0x300, format!("{fstype}\0").as_bytes());
        put(
            &memory,
            0x400,
            format!("fd={},rootmode=40755,user_id=0,group_id=0{extra}\0", fd.0).as_bytes(),
        );
        block_on(as_task(
            task,
            kernel.mount(
                &caller,
                GuestPtr::NULL,
                GuestPtr::new(0x200),
                GuestPtr::new(0x300),
                MsFlags(0),
                GuestPtr::new(0x400),
            ),
        ))
        .unwrap();
        let root_path = kernel.vfs().root();
        let cred = wasixv2_vfs::Cred::root();
        let cx = wasixv2_vfs::Ctx {
            cred: &cred,
            root: &root_path,
            umask: 0,
            task: task.0,
            leases: None,
        };
        let root = block_on(kernel.vfs().lookup(
            &cx,
            &root_path,
            b"/mnt",
            wasixv2_vfs::LookupFlags::follow(),
        ))
        .unwrap()
        .dentry
        .node()
        .clone();
        Self {
            kernel,
            task,
            memory,
            fd,
            root,
        }
    }
    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }
    fn raw_read(&self) -> Vec<u8> {
        let n = now(self
            .kernel
            .read(&self.caller(), self.fd, UserSliceMut::new(BUF, 65536)))
        .unwrap();
        get(&self.memory, BUF, n as usize)
    }
    fn read(&self) -> Vec<u8> {
        loop {
            let r = self.raw_read();
            if !matches!(n32(&r, 4), 2 | 42) {
                return r;
            }
        }
    }
    fn write(&self, b: &[u8]) -> SysResult<u32> {
        put(&self.memory, BUF, b);
        now(self
            .kernel
            .write(&self.caller(), self.fd, UserSlice::new(BUF, b.len() as u32)))
    }
    fn reply(&self, req: &[u8], error: i32, body: &[u8]) -> SysResult<u32> {
        let mut b = vec![0; 16];
        p32(&mut b, 0, (16 + body.len()) as u32);
        p32(&mut b, 4, error as u32);
        p64(&mut b, 8, n64(req, 8));
        b.extend(body);
        self.write(&b)
    }
    fn reply_to(&self, fd: Fd, req: &[u8], error: i32, body: &[u8]) -> SysResult<u32> {
        let mut out = vec![0; 16];
        p32(&mut out, 0, (16 + body.len()) as u32);
        p32(&mut out, 4, error as u32);
        p64(&mut out, 8, n64(req, 8));
        out.extend(body);
        put(&self.memory, BUF, &out);
        now(self
            .kernel
            .write(&self.caller(), fd, UserSlice::new(BUF, out.len() as u32)))
    }
    fn init(&self) {
        self.init_flags((1 << 5) | (1 << 13));
    }
    fn init_flags(&self, flags: u32) {
        let req = self.read();
        assert_eq!(n32(&req, 4), 26);
        assert_eq!(n32(&req, 40), 7);
        assert_eq!(n32(&req, 52) & (1 << 16), 0);
        let mut body = vec![0; 64];
        p32(&mut body, 0, 7);
        p32(&mut body, 4, 46);
        p32(&mut body, 12, flags);
        p32(&mut body, 20, 65536);
        self.reply(&req, 0, &body).unwrap();
    }
    fn exchange<F: Future>(
        &self,
        future: F,
        opcode: u32,
        answer: impl FnOnce(&[u8]) -> (i32, Vec<u8>),
    ) -> F::Output {
        block_on(as_task(self.task, async {
            let mut future = std::pin::pin!(future);
            assert!(futures::poll!(&mut future).is_pending());
            let req = self.read();
            assert_eq!(n32(&req, 4), opcode);
            assert_ne!(n64(&req, 8), 0);
            let (error, body) = answer(&req);
            self.reply(&req, error, &body).unwrap();
            now(future)
        }))
    }
    fn drive<F: Future>(
        &self,
        task: TaskId,
        future: F,
        mut answer: impl FnMut(&[u8]) -> (i32, Vec<u8>),
    ) -> F::Output {
        block_on(as_task(task, async {
            let mut future = std::pin::pin!(future);
            for _ in 0..64 {
                if let std::task::Poll::Ready(result) = futures::poll!(&mut future) {
                    return result;
                }
                let req = self.read();
                let (error, body) = answer(&req);
                self.reply(&req, error, &body).unwrap();
            }
            panic!("daemon did not finish after 64 exchanges")
        }))
    }
    fn client(&self, uid: u32) -> TaskId {
        let task = self
            .kernel
            .spawn(ProcessConfig {
                args: vec!["client".into()],
                env: Vec::new(),
                stdio: Stdio {
                    stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                    stdout: Arc::new(Capture::default()),
                    stderr: Arc::new(Capture::default()),
                },
            })
            .unwrap();
        let caller = Caller::new(task, &self.memory);
        block_on(self.kernel.setresgid(&caller, uid, uid, uid)).unwrap();
        block_on(self.kernel.setresuid(&caller, uid, uid, uid)).unwrap();
        task
    }
    fn path(&self, addr: u32, text: &str) -> Cstr {
        put(&self.memory, addr, format!("{text}\0").as_bytes());
        GuestPtr::new(addr)
    }
    fn lookup(&self, id: u64, kind: FileType) -> Arc<dyn Node> {
        self.exchange(self.root.lookup(b"file"), 1, |r| {
            assert_eq!(&r[40..], b"file\0");
            (0, entry(id, kind))
        })
        .unwrap()
    }
    fn notify(&self, code: u32, body: &[u8]) {
        let mut b = vec![0; 16];
        p32(&mut b, 0, (16 + body.len()) as u32);
        p32(&mut b, 4, code);
        b.extend(body);
        self.write(&b).unwrap();
    }
}

#[test]
fn fuse_init_lookup_cache_and_notifications() {
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    assert_eq!(block_on(node.getattr()).unwrap().size, 64);
    assert_eq!(block_on(d.root.lookup(b"file")).unwrap().ino(), 2);
    assert_eq!(
        d.kernel.vfs().root().dentry.node().file_type(),
        FileType::Directory
    );
    assert_eq!(
        block_on(
            d.kernel
                .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536))
        ),
        Err(SysErr::Errno(Errno::EAGAIN))
    );
    let mut inv = vec![0; 24];
    p64(&mut inv, 0, 2);
    d.notify(2, &inv);
    d.exchange(node.getattr(), 3, |r| {
        assert_eq!(n64(r, 16), 2);
        (0, attr_out(2, FileType::Regular))
    })
    .unwrap();
    let mut inv = vec![0; 16];
    p64(&mut inv, 0, 1);
    p32(&mut inv, 8, 4);
    inv.extend(b"file\0");
    d.notify(3, &inv);
    d.lookup(2, FileType::Regular);
}

#[test]
fn fuse_namespace_and_metadata_protocol() {
    let d = Daemon::new();
    d.init();
    for (kind, opcode) in [
        (NewKind::Regular, 8),
        (NewKind::Directory, 9),
        (NewKind::Symlink(b"target"), 6),
    ] {
        let new = NewNode {
            kind,
            mode: 0o750,
            uid: 0,
            gid: 0,
            task: d.task.0,
        };
        d.exchange(d.root.create(b"new", &new), opcode, |r| {
            if opcode == 6 {
                assert_eq!(&r[40..], b"new\0target\0");
            }
            (0, entry(10 + u64::from(opcode), kind.file_type()))
        })
        .unwrap();
    }
    let node = d.lookup(2, FileType::Regular);
    d.exchange(
        node.setattr(&SetAttr {
            size: Some(123),
            mode: Some(0o640),
            ..SetAttr::default()
        }),
        4,
        |r| {
            assert_eq!(n32(r, 40) & 9, 9);
            assert_eq!(n64(r, 56), 123);
            assert_eq!(n32(r, 108) & 0o777, 0o640);
            (0, attr_out(2, FileType::Regular))
        },
    )
    .unwrap();
    d.exchange(d.root.link(b"hard", &node), 13, |r| {
        assert_eq!(n64(r, 40), 2);
        (0, entry(2, FileType::Regular))
    })
    .unwrap();
    d.exchange(
        d.root
            .rename(b"hard", &d.root, b"renamed", RenameMode::Replace),
        12,
        |r| {
            assert_eq!(n64(r, 40), 1);
            assert_eq!(&r[48..], b"hard\0renamed\0");
            (0, vec![])
        },
    )
    .unwrap();
    d.exchange(
        d.root.rename(b"a", &d.root, b"b", RenameMode::NoReplace),
        45,
        |r| {
            assert_eq!(n32(r, 48), 1);
            (0, vec![])
        },
    )
    .unwrap();
    d.exchange(d.root.unlink(b"file"), 10, |_| (0, vec![]))
        .unwrap();
    d.exchange(d.root.rmdir(b"new"), 11, |_| (0, vec![]))
        .unwrap();
    d.exchange(node.readlink(), 5, |_| (0, b"target".to_vec()))
        .unwrap();
}

#[test]
fn fuse_io_xattrs_and_release() {
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let handle = d
        .exchange(
            node.open(OpenOptions {
                read: true,
                write: true,
                ..OpenOptions::default()
            }),
            14,
            |_| {
                let mut out = vec![0; 16];
                p64(&mut out, 0, 77);
                (0, out)
            },
        )
        .unwrap();
    let mut buf = [0; 8];
    assert_eq!(
        d.exchange(handle.read_at(&mut buf, 3), 15, |r| {
            assert_eq!(n64(r, 40), 77);
            assert_eq!(n64(r, 48), 3);
            (0, b"data".to_vec())
        }),
        Ok(4)
    );
    assert_eq!(&buf[..4], b"data");
    assert_eq!(
        d.exchange(handle.write_at(b"hello", 8), 16, |r| {
            assert_eq!(&r[80..], b"hello");
            let mut b = vec![0; 8];
            p32(&mut b, 0, 5);
            (0, b)
        }),
        Ok(5)
    );
    d.exchange(handle.sync(true), 20, |r| {
        assert_eq!(n32(r, 48), 1);
        (0, vec![])
    })
    .unwrap();
    d.exchange(handle.allocate(AllocMode::PunchHole, 8, 16), 43, |r| {
        assert_eq!(n32(r, 64), 3);
        (0, vec![])
    })
    .unwrap();
    assert_eq!(
        d.exchange(handle.seek_hole(1, 64, SeekHole::Data), 46, |_| (
            0,
            9u64.to_le_bytes().to_vec()
        )),
        Ok(9)
    );
    d.exchange(
        node.setxattr(b"user.key", b"value", XattrSet::Create),
        21,
        |r| {
            assert_eq!(&r[48..], b"user.key\0value");
            (0, vec![])
        },
    )
    .unwrap();
    assert_eq!(
        d.exchange(node.getxattr(b"user.key"), 22, |_| (0, b"value".to_vec()))
            .unwrap(),
        b"value"
    );
    assert_eq!(
        d.exchange(node.listxattr(), 23, |_| (0, b"user.key\0".to_vec()))
            .unwrap(),
        [b"user.key".to_vec()]
    );
    d.exchange(node.removexattr(b"user.key"), 24, |_| (0, vec![]))
        .unwrap();
    d.exchange(handle.flush(99), 25, |r| {
        assert_eq!(n64(r, 56), 99);
        (0, vec![])
    })
    .unwrap();
    drop(handle);
    let req = d.read();
    assert_eq!(n32(&req, 4), 18);
    assert_eq!(n64(&req, 40), 77);
    d.reply(&req, 0, &[]).unwrap();
}

#[test]
fn fuse_malformed_reply_completes_request_with_io_error() {
    let d = Daemon::new();
    d.init();
    block_on(as_task(d.task, async {
        let future = d.root.lookup(b"bad");
        let mut future = std::pin::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        let req = d.read();
        assert_eq!(d.reply(&req, 0, &[0; 8]), Err(SysErr::Errno(Errno::EINVAL)));
        assert!(matches!(future.await, Err(Errno::EIO)));
    }));
    let mut bad = vec![0; 16];
    p32(&mut bad, 0, 17);
    assert_eq!(d.write(&bad), Err(SysErr::Errno(Errno::EINVAL)));
    p32(&mut bad, 0, 16);
    p64(&mut bad, 8, 1234);
    assert_eq!(d.write(&bad), Err(SysErr::Errno(Errno::ENOENT)));
}

#[test]
fn fuse_clone_and_last_device_close_abort_waiters() {
    let d = Daemon::new();
    d.init();
    put(&d.memory, 0x100, b"/dev/fuse\0");
    let cloned = block_on(d.kernel.openat(
        &d.caller(),
        CWD,
        GuestPtr::new(0x100),
        OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK,
        0,
    ))
    .unwrap();
    put(&d.memory, 0x200, &(d.fd.0 as u32).to_le_bytes());
    assert_eq!(
        block_on(
            d.kernel
                .ioctl(&d.caller(), cloned, IoctlRequest::FUSE_DEV_IOC_CLONE, 0x200)
        ),
        Ok(0)
    );
    assert_eq!(block_on(d.kernel.close(&d.caller(), d.fd)), Ok(0));
    block_on(as_task(d.task, async {
        let future = d.root.lookup(b"blocked");
        let mut future = std::pin::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        assert_eq!(d.kernel.close(&d.caller(), cloned).await, Ok(0));
        assert!(matches!(future.await, Err(Errno::ECONNABORTED)));
    }));
}

impl Daemon {
    fn client_file(&self) -> (TaskId, Fd) {
        self.exchange(self.root.getattr(), 3, |_| {
            (0, attr_out(1, FileType::Directory))
        })
        .unwrap();
        let _node = self.lookup(2, FileType::Regular);
        let task = self
            .kernel
            .spawn(ProcessConfig {
                args: vec!["client".into()],
                env: Vec::new(),
                stdio: Stdio {
                    stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                    stdout: Arc::new(Capture::default()),
                    stderr: Arc::new(Capture::default()),
                },
            })
            .unwrap();
        put(&self.memory, 0x100, b"/mnt/file\0");
        let caller = Caller::new(task, &self.memory);
        let fd = self
            .exchange(
                as_task(
                    task,
                    self.kernel
                        .openat(&caller, CWD, GuestPtr::new(0x100), OpenFlags::O_RDWR, 0),
                ),
                14,
                |_| {
                    let mut out = vec![0; 16];
                    p64(&mut out, 0, 77);
                    (0, out)
                },
            )
            .unwrap();
        (task, fd)
    }
}

#[test]
fn fuse_normal_exit_waits_for_flush_then_queues_release() {
    let d = Daemon::new();
    d.init();
    let (task, _fd) = d.client_file();
    let caller = Caller::new(task, &d.memory);
    d.exchange(as_task(task, d.kernel.exit_group(&caller, 0)), 25, |req| {
        assert_eq!(n64(req, 40), 77);
        (0, Vec::new())
    })
    .unwrap();
    let release = d.read();
    assert_eq!(n32(&release, 4), 18);
    d.reply(&release, 0, &[]).unwrap();
    d.kernel.release(task);
}

#[test]
fn fuse_fatal_release_does_not_wait_for_flush() {
    let d = Daemon::new();
    d.init();
    let (task, _fd) = d.client_file();
    d.kernel.fault(task, wasixv2_abi::Signal::SIGSEGV.0);
    let _ = d.kernel.take_pending(task);
    d.kernel.release(task);
    let flush = d.read();
    assert_eq!(n32(&flush, 4), 25);
    // The interrupted exchange has no task waiter; completion still retires it safely.
    d.reply(&flush, 0, &[]).unwrap();
    let release = d.read();
    assert_eq!(n32(&release, 4), 18);
    d.reply(&release, 0, &[]).unwrap();
    assert!(!d.kernel.state.read_or_recover().tasks.contains_key(&task));
}

#[derive(Default)]
struct Listing {
    limit: usize,
    entries: Vec<(Vec<u8>, u64, u8)>,
}
impl wasixv2_vfs::DirSink for Listing {
    fn push(&mut self, e: wasixv2_vfs::DirEntry<'_>) -> bool {
        self.push_type(e, e.file_type.dirent_type())
    }
    fn push_type(&mut self, e: wasixv2_vfs::DirEntry<'_>, dtype: u8) -> bool {
        if self.entries.len() == self.limit {
            return false;
        }
        self.entries.push((e.name.to_vec(), e.next, dtype));
        true
    }
}
fn dirent(name: &[u8], id: u64, next: u64, dtype: u32, plus: bool) -> Vec<u8> {
    let mut out = if plus {
        entry(id, FileType::Regular)
    } else {
        Vec::new()
    };
    let start = out.len();
    out.resize(start + 24, 0);
    p64(&mut out, start, id);
    p64(&mut out, start + 8, next);
    p32(&mut out, start + 16, name.len() as u32);
    p32(&mut out, start + 20, dtype);
    out.extend(name);
    out.resize(out.len().next_multiple_of(8), 0);
    out
}
#[test]
fn fuse_readdir_paginates_and_maps_large_cookies() {
    let d = Daemon::new();
    d.init();
    let dir = d.lookup(2, FileType::Directory);
    let handle = d
        .exchange(
            dir.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            27,
            |_| (0, vec![0; 16]),
        )
        .unwrap();
    let mut sink = Listing {
        limit: 1,
        ..Listing::default()
    };
    let large = 1u64 << 55;
    d.exchange(handle.readdir(0, &mut sink), 44, |_| {
        let mut a = dirent(b"one", 3, large, 0, true);
        a.extend(dirent(b"two", 4, large + 1, 8, true));
        (0, a)
    })
    .unwrap();
    assert_eq!(sink.entries[0].0, b"one");
    assert_eq!(sink.entries[0].2, 0);
    let cookie = sink.entries[0].1;
    assert!(cookie < 100);
    // Both lookup references from the full reply must be forgotten, including "two".
    let n = now(d
        .kernel
        .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536)))
    .unwrap();
    let forget = get(&d.memory, BUF, n as usize);
    assert_eq!(n32(&forget, 4), 42);
    assert_eq!(n32(&forget, 40), 2);
    let mut sink = Listing {
        limit: 10,
        ..Listing::default()
    };
    block_on(as_task(d.task, async {
        let future = handle.readdir(cookie, &mut sink);
        let mut future = std::pin::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        let req = d.read();
        assert_eq!(n64(&req, 48), large);
        d.reply(&req, 0, &dirent(b"two", 0, large + 1, 8, true))
            .unwrap();
        assert!(futures::poll!(&mut future).is_pending());
        let req = d.read();
        assert_eq!(n64(&req, 48), large + 1);
        d.reply(&req, 0, &[]).unwrap();
        future.await.unwrap();
    }));
    assert_eq!(sink.entries.len(), 1);
    assert_eq!(sink.entries[0].0, b"two");
}

#[test]
fn fuse_invalid_lookup_attributes_are_forgotten() {
    let d = Daemon::new();
    d.init();
    let result = d.exchange(d.root.lookup(b"invalid"), 1, |_| {
        let mut out = entry(9, FileType::Regular);
        p32(&mut out, 100, 0);
        (0, out)
    });
    assert!(matches!(result, Err(Errno::EIO)));
    let n = now(d
        .kernel
        .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536)))
    .unwrap();
    let out = get(&d.memory, BUF, n as usize);
    assert_eq!(n32(&out, 4), 42);
    assert_eq!(n64(&out, 48), 9);
    assert_eq!(n64(&out, 56), 1);
}

#[test]
fn fuse_poll_supports_synchronous_readiness_consumers_and_notifications() {
    use wasixv2_wait::{PollMask, PollTable};
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let handle = d
        .exchange(
            node.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            14,
            |_| (0, vec![0; 16]),
        )
        .unwrap();
    let mut registered = false;
    let mut register = |_: &wasixv2_wait::WaitQueue, _: PollMask| {
        registered = true;
    };
    assert!(
        handle
            .poll(Some(&mut PollTable::new(PollMask::IN, &mut register)))
            .is_empty()
    );
    assert!(registered);
    let req = d.read();
    assert_eq!(n32(&req, 4), 40);
    let key = n64(&req, 48);
    assert_eq!(n32(&req, 56), 1);
    let mut out = vec![0; 8];
    p32(&mut out, 0, PollMask::IN.0);
    d.reply(&req, 0, &out).unwrap();
    assert!(handle.poll(None).contains(PollMask::IN));
    d.notify(1, &key.to_le_bytes());
    assert!(handle.poll(None).is_empty());
    let req = d.read();
    assert_eq!(n32(&req, 4), 40);
    p32(&mut out, 0, 0);
    d.reply(&req, 0, &out).unwrap();
    assert!(handle.poll(None).is_empty());
    // The async preflight shares this same queue, including ENOSYS's permanent fallback.
    d.notify(1, &key.to_le_bytes());
    d.exchange(handle.prepare_poll(PollMask::IN.0), 40, |_| {
        (-Errno::ENOSYS.0, Vec::new())
    });
    assert_eq!(handle.poll(None), PollMask::DEFAULT);
    d.notify(1, &key.to_le_bytes());
    assert_eq!(handle.poll(None), PollMask::DEFAULT);
}

#[test]
fn fuse_large_xattr_request_is_delivered_as_one_device_record() {
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let value = vec![0x51; 65536];
    block_on(as_task(d.task, async {
        let future = node.setxattr(b"user.large", &value, XattrSet::Any);
        let mut future = std::pin::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        let n = d
            .kernel
            .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 100000))
            .await
            .unwrap();
        assert!(n > 65536);
        let req = get(&d.memory, BUF, n as usize);
        assert_eq!(n32(&req, 4), 21);
        assert_eq!(&req[59..], &value);
        d.reply(&req, 0, &[]).unwrap();
        assert_eq!(now(future), Ok(()));
    }));
}

#[test]
fn fuse_device_copy_fault_finishes_the_request() {
    let d = Daemon::new();
    d.init();
    block_on(as_task(d.task, async {
        let future = d.root.lookup(b"fault");
        let mut future = std::pin::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        assert_eq!(
            d.kernel
                .read(&d.caller(), d.fd, UserSliceMut::new(256 * 1024 - 8, 65536))
                .await,
            Err(SysErr::Errno(Errno::EFAULT))
        );
        assert!(matches!(now(future), Err(Errno::EIO)));
    }));
}

#[test]
fn fuse_init_retries_a_newer_protocol_major() {
    let d = Daemon::new();
    let req = d.read();
    let mut out = vec![0; 8];
    p32(&mut out, 0, 8);
    d.reply(&req, 0, &out).unwrap();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    assert_eq!(block_on(node.getattr()).unwrap().ino, 2);
}

#[test]
fn fuse_cloned_devices_keep_separate_processing_lists() {
    let d = Daemon::new();
    d.init();
    put(&d.memory, 0x100, b"/dev/fuse\0");
    let cloned = block_on(d.kernel.openat(
        &d.caller(),
        CWD,
        GuestPtr::new(0x100),
        OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK,
        0,
    ))
    .unwrap();
    put(&d.memory, 0x200, &d.fd.0.to_le_bytes());
    block_on(
        d.kernel
            .ioctl(&d.caller(), cloned, IoctlRequest::FUSE_DEV_IOC_CLONE, 0x200),
    )
    .unwrap();
    block_on(as_task(d.task, async {
        let future = d.root.lookup(b"separate");
        let mut future = std::pin::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        let req = d.read();
        let body = entry(2, FileType::Regular);
        let mut reply = vec![0; 16];
        p32(&mut reply, 0, 144);
        p64(&mut reply, 8, n64(&req, 8));
        reply.extend(body);
        put(&d.memory, BUF, &reply);
        assert_eq!(
            d.kernel
                .write(&d.caller(), cloned, UserSlice::new(BUF, 144))
                .await,
            Err(SysErr::Errno(Errno::ENOENT))
        );
        d.reply(&req, 0, &entry(2, FileType::Regular)).unwrap();
        let _node = now(future).unwrap();
        let future = d.root.lookup(b"closing");
        let mut future = std::pin::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        d.kernel
            .read(&d.caller(), cloned, UserSliceMut::new(BUF, 65536))
            .await
            .unwrap();
        d.kernel.close(&d.caller(), cloned).await.unwrap();
        assert!(matches!(now(future), Err(Errno::ECONNABORTED)));
    }));
}

#[test]
fn fuse_subtype_is_reported_in_mounts() {
    let d = Daemon::with_mount("fuse.sshfs", "");
    d.init();
    let root = d.kernel.vfs().root();
    let cred = wasixv2_vfs::Cred::root();
    let cx = wasixv2_vfs::Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: d.task.0,
        leases: None,
    };
    let path = block_on(d.kernel.vfs().lookup(
        &cx,
        &root,
        b"/mnt",
        wasixv2_vfs::LookupFlags::follow(),
    ))
    .unwrap();
    assert_eq!(path.mnt.sb().fs.type_name(), b"fuse.sshfs");
}

#[test]
fn fuse_umount_waits_for_destroy_and_closes_the_connection() {
    let d = Daemon::new();
    d.init();
    let path = d.path(0x100, "/mnt");
    d.exchange(
        d.kernel
            .umount2(&d.caller(), path, wasixv2_abi::UmountFlags(0)),
        38,
        |req| {
            assert_eq!(n32(req, 24), 0);
            (0, Vec::new())
        },
    )
    .unwrap();
    assert_eq!(
        now(d
            .kernel
            .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536))),
        Err(SysErr::Errno(Errno::ENODEV))
    );
}

#[test]
fn fuse_bind_mount_keeps_the_connection_alive() {
    let d = Daemon::new();
    d.init();
    let bind = d.path(0x100, "/bind");
    block_on(d.kernel.mkdirat(&d.caller(), CWD, bind, 0o755)).unwrap();
    let source = d.path(0x100, "/mnt");
    let target = d.path(0x200, "/bind");
    block_on(d.kernel.mount(
        &d.caller(),
        source,
        target,
        GuestPtr::NULL,
        MsFlags::MS_BIND,
        GuestPtr::NULL,
    ))
    .unwrap();
    let at = d.path(0x100, "/mnt");
    assert_eq!(
        now(d
            .kernel
            .umount2(&d.caller(), at, wasixv2_abi::UmountFlags(0))),
        Ok(0)
    );
    assert_eq!(
        now(d
            .kernel
            .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536))),
        Err(SysErr::Errno(Errno::EAGAIN))
    );
    let at = d.path(0x100, "/bind");
    d.exchange(
        d.kernel
            .umount2(&d.caller(), at, wasixv2_abi::UmountFlags(0)),
        38,
        |_| (0, vec![]),
    )
    .unwrap();
}

#[test]
fn fuse_access_and_permissions_use_the_callers_identity() {
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let other = d.client(1);
    assert_eq!(block_on(as_task(other, node.getattr())), Err(Errno::EACCES));
    d.exchange(d.root.getattr(), 3, |_| {
        (0, attr_out(1, FileType::Directory))
    })
    .unwrap();
    let path = d.path(0x100, "/mnt/file");
    d.exchange(
        d.kernel.faccessat2(
            &d.caller(),
            CWD,
            path,
            wasixv2_abi::AccessMode(0),
            wasixv2_abi::AtFlags(0),
        ),
        34,
        |req| {
            assert_eq!(n32(req, 40), 0);
            (0, vec![])
        },
    )
    .unwrap();
}

#[test]
fn fuse_default_permissions_are_checked_locally() {
    let d = Daemon::with_mount("fuse", ",allow_other,default_permissions");
    d.init();
    let other = d.client(1);
    let caller = Caller::new(other, &d.memory);
    let path = d.path(0x100, "/mnt/file");
    let result = d.drive(
        other,
        d.kernel.openat(&caller, CWD, path, OpenFlags::O_RDONLY, 0),
        |r| match n32(r, 4) {
            3 => {
                let id = n64(r, 16);
                let mut a = attr_out(
                    id,
                    if id == 1 {
                        FileType::Directory
                    } else {
                        FileType::Regular
                    },
                );
                if id == 2 {
                    p32(&mut a, 76, FileType::Regular.mode_bits() | 0o600);
                }
                (0, a)
            }
            1 => {
                let mut a = entry(2, FileType::Regular);
                p32(&mut a, 100, FileType::Regular.mode_bits() | 0o600);
                (0, a)
            }
            op => panic!("local DAC unexpectedly sent opcode {op}"),
        },
    );
    assert_eq!(result, Err(SysErr::Errno(Errno::EACCES)));
}

fn named_acl(permission: u16) -> Vec<u8> {
    let mut out = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (1u16, 6u16, u32::MAX),
        (2, permission, 1),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        out.extend(tag.to_le_bytes());
        out.extend(perm.to_le_bytes());
        out.extend(id.to_le_bytes());
    }
    out
}
#[test]
fn fuse_posix_acl_negotiation_and_remote_invalidation() {
    let d = Daemon::with_mount("fuse", ",allow_other");
    d.init_flags((1 << 5) | (1 << 13) | (1 << 20));
    let task = d.client(1);
    let caller = Caller::new(task, &d.memory);
    let path = d.path(0x100, "/mnt/file");
    let mut child_acls = 0;
    let fd = d
        .drive(
            task,
            d.kernel.openat(&caller, CWD, path, OpenFlags::O_RDONLY, 0),
            |r| match n32(r, 4) {
                3 => (0, attr_out(n64(r, 16), FileType::Directory)),
                1 => {
                    let mut a = entry(2, FileType::Regular);
                    p32(&mut a, 100, FileType::Regular.mode_bits() | 0o640);
                    (0, a)
                }
                22 => {
                    if n64(r, 16) == 1 {
                        (-Errno::ENODATA.0, vec![])
                    } else {
                        child_acls += 1;
                        (0, named_acl(4))
                    }
                }
                14 => (0, vec![0; 16]),
                op => panic!("unexpected ACL opcode {op}"),
            },
        )
        .unwrap();
    assert_eq!(child_acls, 1);
    let mut inv = vec![0; 24];
    p64(&mut inv, 0, 2);
    d.notify(2, &inv);
    let path = d.path(0x100, "/mnt/file");
    let result = d.drive(
        task,
        d.kernel.openat(&caller, CWD, path, OpenFlags::O_RDONLY, 0),
        |r| match n32(r, 4) {
            3 => {
                let mut a = attr_out(2, FileType::Regular);
                p32(&mut a, 76, FileType::Regular.mode_bits() | 0o640);
                (0, a)
            }
            22 => {
                child_acls += 1;
                (0, named_acl(0))
            }
            op => panic!("denied ACL sent opcode {op}"),
        },
    );
    assert_eq!(result, Err(SysErr::Errno(Errno::EACCES)));
    assert_eq!(child_acls, 2);
    d.exchange(as_task(task, d.kernel.close(&caller, fd)), 25, |_| {
        (0, vec![])
    })
    .unwrap();
}

#[test]
fn fuse_duplicate_descriptors_flush_each_close_and_release_once() {
    let d = Daemon::new();
    d.init();
    let (task, fd) = d.client_file();
    let caller = Caller::new(task, &d.memory);
    let duplicate = block_on(d.kernel.dup(&caller, fd)).unwrap();
    d.exchange(as_task(task, d.kernel.close(&caller, fd)), 25, |_| {
        (0, vec![])
    })
    .unwrap();
    assert_eq!(
        now(d
            .kernel
            .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536))),
        Err(SysErr::Errno(Errno::EAGAIN))
    );
    d.exchange(
        as_task(task, d.kernel.close(&caller, duplicate)),
        25,
        |_| (0, vec![]),
    )
    .unwrap();
    let release = d.read();
    assert_eq!(n32(&release, 4), 18);
}

#[test]
fn fuse_partial_copy_file_range_does_not_fall_back_after_success() {
    use wasixv2_abi::{InotifyInitFlags, InotifyMask};
    let d = Daemon::new();
    d.init();
    let (task, fd) = d.client_file();
    let caller = Caller::new(task, &d.memory);
    let inotify = block_on(
        d.kernel
            .inotify_init1(&caller, InotifyInitFlags::IN_NONBLOCK),
    )
    .unwrap();
    let path = d.path(0x100, "/mnt/file");
    assert_eq!(
        block_on(as_task(
            task,
            d.kernel
                .inotify_add_watch(&caller, inotify, path, InotifyMask::IN_MODIFY)
        )),
        Ok(1)
    );
    put(&d.memory, 0x800, &0i64.to_le_bytes());
    put(&d.memory, 0x808, &32i64.to_le_bytes());
    let result = d.exchange(
        as_task(
            task,
            d.kernel.copy_file_range(
                &caller,
                fd,
                GuestPtrMut::new(0x800),
                fd,
                GuestPtrMut::new(0x808),
                8,
                0,
            ),
        ),
        47,
        |req| {
            assert_eq!(n64(req, 80), 8);
            (0, 2u64.to_le_bytes().to_vec())
        },
    );
    assert_eq!(result, Ok(2));
    assert_eq!(get(&d.memory, 0x808, 8), 34i64.to_le_bytes());
    let n = block_on(
        d.kernel
            .read(&caller, inotify, UserSliceMut::new(0x900, 64)),
    )
    .unwrap();
    assert!(n >= 16);
    assert_eq!(
        n32(&get(&d.memory, 0x900, n as usize), 4),
        InotifyMask::IN_MODIFY.0
    );
}

#[test]
fn fuse_create_open_directory_sync_statfs_and_mmap_policy() {
    let d = Daemon::new();
    d.init();
    let new = NewNode {
        kind: NewKind::Regular,
        mode: 0o640,
        uid: 0,
        gid: 0,
        task: d.task.0,
    };
    let (node, handle) = d
        .exchange(
            d.root.create_open(
                b"created",
                &new,
                OpenOptions {
                    read: true,
                    write: true,
                    ..OpenOptions::default()
                },
            ),
            35,
            |_| {
                let mut a = entry(2, FileType::Regular);
                let mut opened = vec![0; 16];
                p64(&mut opened, 0, 91);
                p32(&mut opened, 8, 1);
                a.extend(opened);
                (0, a)
            },
        )
        .unwrap();
    assert_eq!(handle.mmap(false), Ok(()));
    assert_eq!(handle.mmap(true), Err(Errno::ENODEV));
    d.exchange(handle.set_size(99), 4, |r| {
        assert_ne!(n32(r, 40) & 64, 0);
        assert_eq!(n64(r, 48), 91);
        (0, attr_out(2, FileType::Regular))
    })
    .unwrap();
    let dir = d
        .exchange(
            d.root.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            27,
            |_| (0, vec![0; 16]),
        )
        .unwrap();
    d.exchange(dir.sync(false), 30, |_| (0, vec![])).unwrap();
    drop(dir);
    let release = d.read();
    assert_eq!(n32(&release, 4), 29);
    d.reply(&release, 0, &[]).unwrap();
    let root = d.kernel.vfs().root();
    let cred = wasixv2_vfs::Cred::root();
    let cx = wasixv2_vfs::Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: d.task.0,
        leases: None,
    };
    let path = block_on(d.kernel.vfs().lookup(
        &cx,
        &root,
        b"/mnt",
        wasixv2_vfs::LookupFlags::follow(),
    ))
    .unwrap();
    let stat = d
        .exchange(path.mnt.sb().fs.statfs(), 17, |_| {
            let mut a = vec![0; 80];
            p64(&mut a, 0, 100);
            p32(&mut a, 40, 4096);
            p32(&mut a, 44, 255);
            (0, a)
        })
        .unwrap();
    assert_eq!(stat.blocks, 100);
    d.exchange(path.mnt.sb().fs.sync(), 50, |_| (0, vec![]))
        .unwrap();
    drop(node);
}

#[test]
fn fuse_cancelled_pending_requests_are_removed() {
    let d = Daemon::new();
    d.init();
    block_on(as_task(d.task, async {
        let mut future = Box::pin(d.root.lookup(b"cancel"));
        assert!(futures::poll!(&mut future).is_pending());
        drop(future);
        assert_eq!(
            d.kernel
                .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536))
                .await,
            Err(SysErr::Errno(Errno::EAGAIN))
        );
    }));
}

#[test]
fn fuse_interrupt_and_late_lookup_reply_balance_forgets() {
    let d = Daemon::new();
    d.init();
    block_on(as_task(d.task, async {
        let mut future = Box::pin(d.root.lookup(b"cancel"));
        assert!(futures::poll!(&mut future).is_pending());
        let req = d.read();
        drop(future);
        let interrupt = d.read();
        assert_eq!(n32(&interrupt, 4), 36);
        assert_eq!(n64(&interrupt, 8), n64(&req, 8) | 1);
        assert_eq!(n64(&interrupt, 40), n64(&req, 8));
        d.reply(&interrupt, -Errno::ENOSYS.0, &[]).unwrap();
        d.reply(&req, 0, &entry(9, FileType::Regular)).unwrap();
        let forget = d.raw_read();
        assert_eq!(n32(&forget, 4), 42);
        assert_eq!(n64(&forget, 48), 9);
        assert_eq!(n64(&forget, 56), 1);
    }));
}

#[test]
fn fuse_release_retains_lookup_until_daemon_completion() {
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let handle = d
        .exchange(
            node.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            14,
            |_| (0, vec![0; 16]),
        )
        .unwrap();
    drop(node);
    drop(handle);
    let release = d.raw_read();
    assert_eq!(n32(&release, 4), 18);
    d.reply(&release, 0, &[]).unwrap();
    let forget = d.raw_read();
    assert_eq!(n32(&forget, 4), 42);
    assert_eq!(n64(&forget, 48), 2);
}

#[test]
fn fuse_last_close_after_lazy_unmount_sends_release_before_destroy() {
    let d = Daemon::new();
    d.init();
    let (task, fd) = d.client_file();
    let at = d.path(0x100, "/mnt");
    now(d
        .kernel
        .umount2(&d.caller(), at, wasixv2_abi::UmountFlags::MNT_DETACH))
    .unwrap();
    let caller = Caller::new(task, &d.memory);
    d.exchange(as_task(task, d.kernel.close(&caller, fd)), 25, |_| {
        (0, vec![])
    })
    .unwrap();
    let release = d.raw_read();
    assert_eq!(n32(&release, 4), 18);
    d.reply(&release, 0, &[]).unwrap();
    let destroy = d.read();
    assert_eq!(n32(&destroy, 4), 38);
    d.reply(&destroy, 0, &[]).unwrap();
}

#[test]
fn fuse_notify_delete_retires_an_open_directory() {
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Directory);
    let mut body = vec![0; 24];
    p64(&mut body, 0, 1);
    p64(&mut body, 8, 2);
    p32(&mut body, 16, 4);
    body.extend(b"file\0");
    d.notify(6, &body);
    assert!(node.unlinked_directory());
    assert_eq!(block_on(node.getattr()).unwrap().nlink, 0);
    let new = NewNode {
        kind: NewKind::Regular,
        mode: 0o600,
        uid: 0,
        gid: 0,
        task: d.task.0,
    };
    assert!(matches!(
        now(node.create(b"child", &new)),
        Err(Errno::ENOENT)
    ));
}

#[test]
fn fuse_invalid_create_reply_releases_before_forgetting() {
    let d = Daemon::new();
    d.init();
    let new = NewNode {
        kind: NewKind::Regular,
        mode: 0o600,
        uid: 0,
        gid: 0,
        task: d.task.0,
    };
    let result = d.exchange(
        d.root.create_open(
            b"bad",
            &new,
            OpenOptions {
                read: true,
                ..OpenOptions::default()
            },
        ),
        35,
        |_| {
            let mut a = entry(9, FileType::Regular);
            p32(&mut a, 100, 0);
            let mut opened = vec![0; 16];
            p64(&mut opened, 0, 88);
            a.extend(opened);
            (0, a)
        },
    );
    assert!(matches!(result, Err(Errno::EIO)));
    let release = d.raw_read();
    assert_eq!(n32(&release, 4), 18);
    assert_eq!(n64(&release, 40), 88);
    d.reply(&release, 0, &[]).unwrap();
    let forget = d.raw_read();
    assert_eq!(n32(&forget, 4), 42);
    assert_eq!(n64(&forget, 48), 9);
}

#[test]
fn fuse_zero_timeouts_revalidate_and_count_repeated_lookups() {
    let d = Daemon::new();
    d.init();
    let node = d
        .exchange(d.root.lookup(b"expiry"), 1, |_| {
            let mut a = entry(2, FileType::Regular);
            p64(&mut a, 16, 0);
            p64(&mut a, 24, 0);
            (0, a)
        })
        .unwrap();
    d.exchange(node.getattr(), 3, |_| (0, attr_out(2, FileType::Regular)))
        .unwrap();
    let again = d
        .exchange(d.root.lookup(b"expiry"), 1, |_| {
            (0, entry(2, FileType::Regular))
        })
        .unwrap();
    drop(node);
    drop(again);
    let forget = d.raw_read();
    assert_eq!(n32(&forget, 4), 42);
    assert_eq!(n64(&forget, 56), 2);
}

#[test]
fn fuse_user_namespace_root_mounts_with_mapped_credentials() {
    let d = Daemon::new();
    d.init();
    let target = d.path(0x100, "/userns");
    block_on(d.kernel.mkdirat(&d.caller(), CWD, target, 0o755)).unwrap();
    block_on(d.kernel.vfs().mount(
        b"/proc",
        d.kernel.procfs(),
        wasixv2_vfs::MountFlags::default(),
        true,
    ))
    .unwrap();
    block_on(d.kernel.setresgid(&d.caller(), 1000, 1000, 1000)).unwrap();
    block_on(d.kernel.setresuid(&d.caller(), 1000, 1000, 1000)).unwrap();
    // set*id clears dumpability; Linux proc map ownership follows it.
    block_on(d.kernel.prctl(
        &d.caller(),
        wasixv2_abi::PrctlOption::PR_SET_DUMPABLE,
        1,
        0,
        0,
        0,
    ))
    .unwrap();
    block_on(d.kernel.unshare(
        &d.caller(),
        (wasixv2_abi::CloneFlags::CLONE_NEWUSER | wasixv2_abi::CloneFlags::CLONE_NEWNS).0 as u32,
    ))
    .unwrap();
    for (path, value) in [
        ("/proc/self/setgroups", "deny"),
        ("/proc/self/uid_map", "0 1000 1"),
        ("/proc/self/gid_map", "0 1000 1"),
    ] {
        let p = d.path(0x100, path);
        let fd = block_on(d.kernel.openat(&d.caller(), CWD, p, OpenFlags::O_WRONLY, 0)).unwrap();
        put(&d.memory, 0x1000, value.as_bytes());
        block_on(
            d.kernel
                .write(&d.caller(), fd, UserSlice::new(0x1000, value.len() as u32)),
        )
        .unwrap();
        block_on(d.kernel.close(&d.caller(), fd)).unwrap();
    }
    let p = d.path(0x100, "/dev/fuse");
    let fd = block_on(d.kernel.openat(
        &d.caller(),
        CWD,
        p,
        OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK,
        0,
    ))
    .unwrap();
    let at = d.path(0x200, "/userns");
    let ty = d.path(0x300, "fuse");
    let opts = d.path(
        0x400,
        &format!("fd={},rootmode=40755,user_id=0,group_id=0", fd.0),
    );
    block_on(as_task(
        d.task,
        d.kernel.mount(
            &d.caller(),
            GuestPtr::NULL,
            at,
            ty,
            MsFlags(0),
            GuestPtr::new(opts.addr()),
        ),
    ))
    .unwrap();
    let p = d.path(0x100, "/userns");
    let location = block_on(d.kernel.openat(
        &d.caller(),
        CWD,
        p,
        OpenFlags::O_PATH | OpenFlags::O_DIRECTORY,
        0,
    ))
    .unwrap();
    let node = d
        .kernel
        .file(d.task, location)
        .unwrap()
        .path()
        .dentry
        .node()
        .clone();
    block_on(d.kernel.close(&d.caller(), location)).unwrap();
    let daemon = Daemon {
        fd,
        root: node,
        ..d
    };
    daemon.init();
    let a = daemon
        .exchange(daemon.root.getattr(), 3, |r| {
            assert_eq!(n32(r, 24), 0);
            assert_eq!(n32(r, 28), 0);
            (0, attr_out(1, FileType::Directory))
        })
        .unwrap();
    assert_eq!(a.uid, 1000);
    assert_eq!(a.gid, 1000);
}

#[test]
fn fusectl_abort_wakes_waiters_and_unmount_removes_connection() {
    let d = Daemon::new();
    d.init();
    block_on(d.kernel.vfs().mount(
        b"/sys",
        d.kernel.sysfs(),
        wasixv2_vfs::MountFlags::default(),
        true,
    ))
    .unwrap();
    let target = d.path(0x100, "/sys/fs/fuse/connections");
    let ty = d.path(0x200, "fusectl");
    block_on(d.kernel.mount(
        &d.caller(),
        GuestPtr::NULL,
        target,
        ty,
        MsFlags(0),
        GuestPtr::NULL,
    ))
    .unwrap();
    let waiting = d.path(0x100, "/sys/fs/fuse/connections/1/waiting");
    let fd = block_on(
        d.kernel
            .openat(&d.caller(), CWD, waiting, OpenFlags::O_RDONLY, 0),
    )
    .unwrap();
    let abort = d.path(0x100, "/sys/fs/fuse/connections/1/abort");
    let abort = block_on(
        d.kernel
            .openat(&d.caller(), CWD, abort, OpenFlags::O_WRONLY, 0),
    )
    .unwrap();
    block_on(as_task(d.task, async {
        let mut future = Box::pin(d.root.lookup(b"blocked"));
        assert!(futures::poll!(&mut future).is_pending());
        let n = d
            .kernel
            .read(&d.caller(), fd, UserSliceMut::new(0x1000, 64))
            .await
            .unwrap();
        assert_eq!(get(&d.memory, 0x1000, n as usize), b"1\n");
        put(&d.memory, 0x1000, b"1");
        assert_eq!(
            d.kernel
                .write(&d.caller(), abort, UserSlice::new(0x1000, 1))
                .await,
            Ok(1)
        );
        assert!(matches!(now(future), Err(Errno::ECONNABORTED)));
    }));
    let at = d.path(0x100, "/mnt");
    now(d
        .kernel
        .umount2(&d.caller(), at, wasixv2_abi::UmountFlags(0)))
    .unwrap();
    let path = d.path(0x100, "/sys/fs/fuse/connections/1");
    assert_eq!(
        block_on(
            d.kernel
                .openat(&d.caller(), CWD, path, OpenFlags::O_PATH, 0)
        ),
        Err(SysErr::Errno(Errno::ENOENT))
    );
}

#[test]
fn fuse_malformed_notifications_and_unknown_inodes_are_rejected() {
    let d = Daemon::new();
    d.init();
    let mut header = vec![0; 40];
    p32(&mut header, 0, 40);
    p32(&mut header, 4, 2);
    p64(&mut header, 16, 999);
    assert_eq!(d.write(&header), Err(SysErr::Errno(Errno::ENOENT)));
    p32(&mut header, 0, 39);
    assert_eq!(d.write(&header), Err(SysErr::Errno(Errno::EINVAL)));
}

#[test]
fn fuse_sync_init_ioctl_delays_mount_until_the_reply() {
    let d = Daemon::new();
    d.init();
    let at = d.path(0x100, "/sync");
    block_on(d.kernel.mkdirat(&d.caller(), CWD, at, 0o755)).unwrap();
    let at = d.path(0x100, "/dev/fuse");
    let fd = block_on(d.kernel.openat(
        &d.caller(),
        CWD,
        at,
        OpenFlags::O_RDWR | OpenFlags::O_NONBLOCK,
        0,
    ))
    .unwrap();
    assert_eq!(
        block_on(
            d.kernel
                .ioctl(&d.caller(), fd, IoctlRequest::FUSE_DEV_IOC_SYNC_INIT, 0)
        ),
        Ok(0)
    );
    let target = d.path(0x100, "/sync");
    let ty = d.path(0x200, "fuse");
    let opts = d.path(
        0x300,
        &format!("fd={},rootmode=40755,user_id=0,group_id=0", fd.0),
    );
    block_on(as_task(d.task, async {
        let caller = d.caller();
        let mut future = Box::pin(d.kernel.mount(
            &caller,
            GuestPtr::NULL,
            target,
            ty,
            MsFlags(0),
            GuestPtr::new(opts.addr()),
        ));
        assert!(futures::poll!(&mut future).is_pending());
        let n = d
            .kernel
            .read(&d.caller(), fd, UserSliceMut::new(BUF, 65536))
            .await
            .unwrap();
        let req = get(&d.memory, BUF, n as usize);
        assert_eq!(n32(&req, 4), 26);
        let mut init = vec![0; 64];
        p32(&mut init, 0, 7);
        p32(&mut init, 4, 46);
        p32(&mut init, 20, 4096);
        d.reply_to(fd, &req, 0, &init).unwrap();
        assert_eq!(now(future), Ok(0));
    }));
    assert_eq!(
        block_on(
            d.kernel
                .ioctl(&d.caller(), fd, IoctlRequest::FUSE_DEV_IOC_SYNC_INIT, 0)
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
}

#[test]
fn fuse_plain_readdir_reads_until_eof() {
    let d = Daemon::new();
    d.init_flags(1 << 5);
    let dir = d.lookup(2, FileType::Directory);
    let handle = d
        .exchange(
            dir.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            27,
            |_| (0, vec![0; 16]),
        )
        .unwrap();
    let mut listing = Listing {
        limit: 10,
        ..Listing::default()
    };
    let mut batches = 0;
    d.drive(d.task, handle.readdir(0, &mut listing), |r| {
        assert_eq!(n32(r, 4), 28);
        batches += 1;
        match batches {
            1 => (0, dirent(b"first", 3, 9, 8, false)),
            2 => {
                assert_eq!(n64(r, 48), 9);
                (0, dirent(b"second", 4, 11, 8, false))
            }
            _ => (0, vec![]),
        }
    })
    .unwrap();
    assert_eq!(listing.entries.len(), 2);
    assert_eq!(batches, 3);
}

#[test]
fn fuse_max_read_splits_requests_without_truncating_a_read() {
    let d = Daemon::with_mount("fuse", ",max_read=4096");
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let handle = d
        .exchange(
            node.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            14,
            |_| (0, vec![0; 16]),
        )
        .unwrap();
    let mut buf = vec![0; 8192];
    let mut calls = 0;
    assert_eq!(
        d.drive(d.task, handle.read_at(&mut buf, 0), |r| {
            assert_eq!(n32(r, 4), 15);
            assert_eq!(n32(r, 56), 4096);
            assert_eq!(n64(r, 48), calls * 4096);
            calls += 1;
            (0, vec![0x51; 4096])
        }),
        Ok(8192)
    );
    assert_eq!(calls, 2);
}

#[test]
fn fuse_open_enosys_and_noflush_flags_are_honored() {
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let handle = d
        .exchange(
            node.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            14,
            |_| (-Errno::ENOSYS.0, vec![]),
        )
        .unwrap();
    drop(handle);
    let handle = now(node.open(OpenOptions {
        read: true,
        ..OpenOptions::default()
    }))
    .unwrap();
    drop(handle);
    for _ in 0..2 {
        let release = d.read();
        assert_eq!(n32(&release, 4), 18);
        assert_eq!(n64(&release, 40), 0);
        d.reply(&release, 0, &[]).unwrap();
    }
    assert_eq!(
        now(d
            .kernel
            .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536))),
        Err(SysErr::Errno(Errno::EAGAIN))
    );
    let d = Daemon::new();
    d.init();
    let node = d.lookup(2, FileType::Regular);
    let handle = d
        .exchange(
            node.open(OpenOptions {
                read: true,
                ..OpenOptions::default()
            }),
            14,
            |_| {
                let mut b = vec![0; 16];
                p32(&mut b, 8, 1 << 5);
                (0, b)
            },
        )
        .unwrap();
    assert_eq!(now(handle.flush(1)), Ok(()));
}

#[test]
fn fuseblk_and_block_size_options_are_refused() {
    let d = Daemon::new();
    d.init();
    let target = d.path(0x100, "/mnt");
    let ty = d.path(0x200, "fuseblk");
    assert_eq!(
        block_on(d.kernel.mount(
            &d.caller(),
            GuestPtr::NULL,
            target,
            ty,
            MsFlags(0),
            GuestPtr::NULL
        )),
        Err(SysErr::Errno(Errno::ENODEV))
    );
    let ty = d.path(0x200, "fuse");
    let opts = d.path(
        0x300,
        &format!(
            "fd={},rootmode=40755,user_id=0,group_id=0,blksize=4096",
            d.fd.0
        ),
    );
    assert_eq!(
        block_on(as_task(
            d.task,
            d.kernel.mount(
                &d.caller(),
                GuestPtr::NULL,
                target,
                ty,
                MsFlags(0),
                GuestPtr::new(opts.addr())
            )
        )),
        Err(SysErr::Errno(Errno::EINVAL))
    );
}

#[test]
fn fuse_epoll_delivers_remote_poll_notifications() {
    use wasixv2_abi::{EpollCreateFlags, EpollCtlOp, EpollEvents};
    let d = Daemon::new();
    d.init();
    let (task, fd) = d.client_file();
    let caller = Caller::new(task, &d.memory);
    let ep = block_on(d.kernel.epoll_create1(&caller, EpollCreateFlags(0))).unwrap();
    put(&d.memory, 0x800, &EpollEvents::EPOLLIN.0.to_le_bytes());
    put(&d.memory, 0x808, &0x1234u64.to_le_bytes());
    let mut key = 0;
    d.exchange(
        as_task(
            task,
            d.kernel.epoll_ctl(
                &caller,
                ep,
                EpollCtlOp::EPOLL_CTL_ADD,
                fd,
                GuestPtr::new(0x800),
            ),
        ),
        40,
        |r| {
            key = n64(r, 48);
            (0, vec![0; 8])
        },
    )
    .unwrap();
    let wait = || {
        d.kernel.epoll_pwait(
            &caller,
            ep,
            GuestPtrMut::new(0x900),
            1,
            0,
            GuestPtr::NULL,
            8,
        )
    };
    assert_eq!(block_on(as_task(task, wait())), Ok(0));
    d.notify(1, &key.to_le_bytes());
    assert_eq!(
        d.exchange(as_task(task, wait()), 40, |_| {
            let mut b = vec![0; 8];
            p32(&mut b, 0, EpollEvents::EPOLLIN.0);
            (0, b)
        }),
        Ok(1)
    );
    let event = get(&d.memory, 0x900, 16);
    assert_eq!(n32(&event, 0), EpollEvents::EPOLLIN.0);
    assert_eq!(n64(&event, 8), 0x1234);
}

#[test]
fn fuse_background_limits_throttle_until_reply_or_control_update() {
    let d = Daemon::new();
    let req = d.read();
    let mut init = vec![0; 64];
    p32(&mut init, 0, 7);
    p32(&mut init, 4, 46);
    p32(&mut init, 16, 1 | (1 << 16));
    p32(&mut init, 20, 4096);
    d.reply(&req, 0, &init).unwrap();
    let node = d.lookup(2, FileType::Regular);
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(
            d.exchange(
                node.open(OpenOptions {
                    read: true,
                    ..OpenOptions::default()
                }),
                14,
                |_| (0, vec![0; 16]),
            )
            .unwrap(),
        );
    }
    drop(handles);
    let first = d.read();
    assert_eq!(n32(&first, 4), 18);
    assert_eq!(
        now(d
            .kernel
            .read(&d.caller(), d.fd, UserSliceMut::new(BUF, 65536))),
        Err(SysErr::Errno(Errno::EAGAIN))
    );
    d.reply(&first, 0, &[]).unwrap();
    let second = d.read();
    assert_eq!(n32(&second, 4), 18);
    block_on(d.kernel.vfs().mount(
        b"/sys",
        d.kernel.sysfs(),
        wasixv2_vfs::MountFlags::default(),
        true,
    ))
    .unwrap();
    let target = d.path(0x100, "/sys/fs/fuse/connections");
    let ty = d.path(0x200, "fusectl");
    block_on(d.kernel.mount(
        &d.caller(),
        GuestPtr::NULL,
        target,
        ty,
        MsFlags(0),
        GuestPtr::NULL,
    ))
    .unwrap();
    let path = d.path(0x100, "/sys/fs/fuse/connections/1/max_background");
    let limit = block_on(
        d.kernel
            .openat(&d.caller(), CWD, path, OpenFlags::O_RDWR, 0),
    )
    .unwrap();
    put(&d.memory, 0x1000, b"0x3\n");
    assert_eq!(
        block_on(
            d.kernel
                .write(&d.caller(), limit, UserSlice::new(0x1000, 4))
        ),
        Ok(4)
    );
    for _ in 0..2 {
        let req = d.read();
        assert_eq!(n32(&req, 4), 18);
        d.reply(&req, 0, &[]).unwrap();
    }
    assert_eq!(
        block_on(
            d.kernel
                .write(&d.caller(), limit, UserSlice::new(0x1000, 4))
        ),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    d.reply(&second, 0, &[]).unwrap();
}
