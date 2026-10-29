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
        put(&memory, 0x300, b"fuse\0");
        put(
            &memory,
            0x400,
            format!("fd={},rootmode=40755,user_id=0,group_id=0\0", fd.0).as_bytes(),
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
    fn read(&self) -> Vec<u8> {
        loop {
            let n = now(self
                .kernel
                .read(&self.caller(), self.fd, UserSliceMut::new(BUF, 65536)))
            .unwrap();
            let request = get(&self.memory, BUF, n as usize);
            // A daemon consumes these one-way messages without sending an out_header.
            if !matches!(n32(&request, 4), 2 | 42) {
                return request;
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
    fn init(&self) {
        let req = self.read();
        assert_eq!(n32(&req, 4), 26);
        assert_eq!(n32(&req, 40), 7);
        assert_eq!(n32(&req, 52) & (1 << 16), 0);
        let mut body = vec![0; 64];
        p32(&mut body, 0, 7);
        p32(&mut body, 4, 46);
        p32(&mut body, 12, (1 << 5) | (1 << 13));
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
            assert_eq!(n32(&req, 24), 0);
            let (error, body) = answer(&req);
            self.reply(&req, error, &body).unwrap();
            future.await
        }))
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
