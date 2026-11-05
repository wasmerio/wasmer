//! BSD process accounting (`kernel/acct.c`): `acct(2)` and the `acct_v3` records exits write.

use wasixv2_abi::{Idtype, SiginfoPtr, WaitOptions};
use wasixv2_vfs::MountFlags;

use super::{
    ipc::{errno, get, put},
    proc::ForkableMemory,
    *,
};

const CWD: Fd = Fd(-100);
const SIGCHLD: u32 = Signal::SIGCHLD.0 as u32;

struct P {
    task: TaskId,
    memory: ForkableMemory,
}

impl P {
    fn new(task: TaskId) -> Self {
        Self {
            task,
            memory: ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536]))),
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    fn path(&self, at: Option<&str>) -> Cstr {
        match at {
            None => GuestPtr::NULL,
            Some(at) => {
                put(&self.memory.0, 0x1000, format!("{at}\0").as_bytes());
                GuestPtr::new(0x1000)
            }
        }
    }

    fn acct(&self, kernel: &Kernel, at: Option<&str>) -> SysResult<i32> {
        block_on(kernel.acct(&self.caller(), self.path(at)))
    }

    fn create(&self, kernel: &Kernel, at: &str) {
        let flags = OpenFlags::O_CREAT | OpenFlags::O_WRONLY;
        let fd = block_on(kernel.openat(&self.caller(), CWD, self.path(Some(at)), flags, 0o600));
        block_on(kernel.close(&self.caller(), fd.unwrap())).unwrap();
    }

    fn contents(&self, kernel: &Kernel, at: &str) -> Vec<u8> {
        let flags = OpenFlags::O_RDONLY;
        let fd =
            block_on(kernel.openat(&self.caller(), CWD, self.path(Some(at)), flags, 0)).unwrap();
        let n = block_on(kernel.read(&self.caller(), fd, UserSliceMut::new(0x4000, 4096))).unwrap();
        block_on(kernel.close(&self.caller(), fd)).unwrap();
        get(&self.memory.0, 0x4000, n as usize)
    }

    fn fork(&self, kernel: &Kernel) -> (P, Pid) {
        let null = GuestPtrMut::NULL;
        let pid = block_on(kernel.clone(&self.caller(), SIGCHLD, 0, null, 0, null)).unwrap();
        let mut child = None;
        while let Some(action) = kernel.take_pending(self.task) {
            if let GuestAction::Fork { child: task, .. } = action {
                child = Some(task);
            }
        }
        (P::new(child.unwrap()), pid)
    }

    fn reap(&self, kernel: &Kernel, pid: Pid) {
        let options = WaitOptions(WaitOptions::WEXITED.0);
        let caller = self.caller();
        let info = SiginfoPtr::new(0x300);
        let wait = kernel.waitid(&caller, Idtype::P_PID, pid, info, options, RusagePtr::NULL);
        assert_eq!(block_on(wait), Ok(0));
    }

    fn exit(&self, kernel: &Kernel, status: i32) {
        block_on(kernel.exit_group(&self.caller(), status)).unwrap();
        block_on(kernel.exit_mm(&self.caller()));
        kernel.release(self.task);
    }
}

fn machine_as(uid: u32) -> (Kernel, P) {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        kernel
            .with_credentials(Credentials::new(uid, uid))
            .with_init()
    });
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

/// An `acct_v3` record's fields: flag, version, exit code, uid, pid, ppid, comm.
fn fields(record: &[u8]) -> (u8, u8, u32, u32, u32, u32, String) {
    let word = |at: usize| u32::from_le_bytes(record[at..at + 4].try_into().unwrap());
    let comm = &record[48..64];
    let len = comm.iter().position(|&b| b == 0).unwrap_or(16);
    (
        record[0],
        record[1],
        word(4),
        word(8),
        word(16),
        word(20),
        String::from_utf8(comm[..len].to_vec()).unwrap(),
    )
}

/// With accounting on, every exit appends a record: how it ended, who it was, its parent and
/// its name; a fork child that never called `execve` is flagged `AFORK`, one a signal killed
/// `AXSIG`. A null name turns accounting off.
#[test]
fn exits_append_acct_v3_records() {
    let (kernel, p) = machine_as(0);
    p.create(&kernel, "/acct");
    assert_eq!(p.acct(&kernel, Some("/acct")), Ok(0));

    let (child, pid) = p.fork(&kernel);
    child.exit(&kernel, 7);
    p.reap(&kernel, pid);
    let records = p.contents(&kernel, "/acct");
    assert_eq!(records.len(), 64);
    let parent = block_on(kernel.getpid(&p.caller())).unwrap() as u32;
    assert_eq!(
        fields(&records),
        (0x01, 3, 7 << 8, 0, pid as u32, parent, "test".into())
    );

    let (child, pid) = p.fork(&kernel);
    let caller = p.caller();
    assert_eq!(
        block_on(kernel.sys_kill(&caller, pid, Signal::SIGKILL)),
        Ok(0)
    );
    assert!(matches!(
        kernel.take_pending(child.task),
        Some(GuestAction::Exit(_))
    ));
    block_on(kernel.exit_mm(&child.caller()));
    kernel.release(child.task);
    p.reap(&kernel, pid);
    let records = p.contents(&kernel, "/acct");
    assert_eq!(records.len(), 128);
    let (flag, _, code, ..) = fields(&records[64..]);
    assert_eq!((flag, code), (0x01 | 0x10, 9));

    assert_eq!(p.acct(&kernel, None), Ok(0));
    let (child, pid) = p.fork(&kernel);
    child.exit(&kernel, 0);
    p.reap(&kernel, pid);
    assert_eq!(p.contents(&kernel, "/acct").len(), 128);
}

/// `acct(2)` needs `CAP_SYS_PACCT`, and a regular file outside procfs.
#[test]
fn acct_takes_a_regular_file_from_the_privileged() {
    let (kernel, p) = machine_as(0);
    assert_eq!(errno(p.acct(&kernel, Some("/nowhere"))), Errno::ENOENT);
    assert_eq!(
        errno(p.acct(&kernel, Some("/proc/self/comm"))),
        Errno::EINVAL
    );
    let dir = p.path(Some("/d"));
    block_on(kernel.mkdirat(&p.caller(), CWD, dir, 0o755)).unwrap();
    assert_eq!(errno(p.acct(&kernel, Some("/d"))), Errno::EISDIR);

    let (kernel, p) = machine_as(1000);
    assert_eq!(errno(p.acct(&kernel, None)), Errno::EPERM);
}

#[test]
fn acct_asu_tracks_successful_capability_use_and_resets_on_fork() {
    use wasixv2_abi::PrctlOption;
    let (kernel, p) = machine_as(0);
    p.create(&kernel, "/acct");
    p.acct(&kernel, Some("/acct")).unwrap(); // parent uses CAP_SYS_PACCT
    let (plain, pid) = p.fork(&kernel);
    plain.exit(&kernel, 0);
    p.reap(&kernel, pid);
    assert_eq!(
        p.contents(&kernel, "/acct")[0] & crate::acct::ASU,
        0,
        "uid 0 alone is not privilege use"
    );
    let (used, pid) = p.fork(&kernel);
    assert_eq!(
        block_on(kernel.prctl(&used.caller(), PrctlOption::PR_GET_IO_FLUSHER, 0, 0, 0, 0)),
        Ok(0)
    );
    let (grandchild, grand_pid) = used.fork(&kernel);
    grandchild.exit(&kernel, 0);
    used.reap(&kernel, grand_pid);
    used.exit(&kernel, 0);
    p.reap(&kernel, pid);
    let records = p.contents(&kernel, "/acct");
    assert_eq!(
        records[64] & crate::acct::ASU,
        0,
        "fork clears inherited PF_SUPERPRIV"
    );
    assert_eq!(records[128] & crate::acct::ASU, crate::acct::ASU);
    // The VFS's DAC override must feed the same accounting flag.
    p.create(&kernel, "/private");
    block_on(kernel.fchownat(
        &p.caller(),
        CWD,
        p.path(Some("/private")),
        99,
        99,
        AtFlags(0),
    ))
    .unwrap();
    block_on(kernel.fchmodat(&p.caller(), CWD, p.path(Some("/private")), 0)).unwrap();
    let (used, pid) = p.fork(&kernel);
    used.contents(&kernel, "/private");
    used.exit(&kernel, 0);
    p.reap(&kernel, pid);
    assert_eq!(
        p.contents(&kernel, "/acct")[192] & crate::acct::ASU,
        crate::acct::ASU
    );
}

struct AcctClock(std::sync::atomic::AtomicU64);
impl Host for AcctClock {
    fn realtime(&self) -> Duration {
        TestHost.realtime()
    }
    fn monotonic(&self) -> Duration {
        Duration::from_secs(self.0.load(std::sync::atomic::Ordering::Relaxed))
    }
    fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async {})
    }
    fn random(&self, bytes: &mut [u8]) {
        TestHost.random(bytes)
    }
    fn executor(&self) -> &dyn Executor {
        &TestHost
    }
}

struct SpaceFs {
    inner: Arc<MemFs>,
    avail: std::sync::atomic::AtomicU64,
    fail: std::sync::atomic::AtomicBool,
}
#[async_trait::async_trait(?Send)]
impl wasixv2_vfs::Filesystem for SpaceFs {
    fn info(&self) -> wasixv2_vfs::FsInfo {
        self.inner.info()
    }
    fn root(&self) -> Arc<dyn wasixv2_vfs::Node> {
        self.inner.root()
    }
    async fn statfs(&self) -> wasixv2_vfs::FsResult<wasixv2_vfs::StatFs> {
        use std::sync::atomic::Ordering::Relaxed;
        if self.fail.load(Relaxed) {
            return Err(Errno::EIO);
        }
        Ok(wasixv2_vfs::StatFs {
            blocks: 100,
            blocks_avail: self.avail.load(Relaxed),
            blocks_free: self.avail.load(Relaxed),
            ..Default::default()
        })
    }
}

#[test]
fn acct_free_space_hysteresis_interval_and_statfs_failure() {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    let clock = Arc::new(AcctClock(AtomicU64::new(0)));
    let root = Arc::new(SpaceFs {
        inner: Arc::new(MemFs::new(Arc::new(HostClock(clock.clone())), 0o755, 0, 0)),
        avail: AtomicU64::new(100),
        fail: AtomicBool::new(false),
    });
    let kernel = Kernel::new(clock.clone(), root.clone())
        .with_credentials(Credentials::new(0, 0))
        .with_init();
    let task = kernel
        .spawn(ProcessConfig {
            args: vec!["acct".into()],
            env: vec![],
            stdio: Stdio {
                stdin: Arc::new(Input(Mutex::new(Vec::new()))),
                stdout: Arc::new(Capture::default()),
                stderr: Arc::new(Capture::default()),
            },
        })
        .unwrap();
    let p = P::new(task);
    block_on(
        kernel
            .vfs
            .mount(b"/proc", kernel.procfs(), MountFlags::default(), true),
    )
    .unwrap();
    assert_eq!(p.contents(&kernel, "/proc/sys/kernel/acct"), b"4\t2\t30\n");
    p.create(&kernel, "/acct");
    p.acct(&kernel, Some("/acct")).unwrap();
    let exit = |seconds, available| {
        clock.0.store(seconds, Relaxed);
        root.avail.store(available, Relaxed);
        let (child, pid) = p.fork(&kernel);
        child.exit(&kernel, 0);
        p.reap(&kernel, pid);
        p.contents(&kernel, "/acct").len()
    };
    assert_eq!(exit(0, 4), 64, "resume is inclusive");
    assert_eq!(exit(1, 1), 128, "no check before the interval");
    assert_eq!(exit(30, 2), 128, "suspend is inclusive");
    assert_eq!(exit(60, 3), 128, "stay paused between thresholds");
    root.fail.store(true, Relaxed);
    assert_eq!(exit(90, 100), 128, "failed statfs preserves paused state");
    root.fail.store(false, Relaxed);
    assert_eq!(exit(90, 4), 192, "failed statfs leaves the check due");
    root.fail.store(true, Relaxed);
    assert_eq!(exit(120, 0), 256, "failed statfs preserves active state");
    root.fail.store(false, Relaxed);
    assert_eq!(exit(120, 0), 256);
    let fd = block_on(kernel.openat(
        &p.caller(),
        CWD,
        p.path(Some("/proc/sys/kernel/acct")),
        OpenFlags::O_WRONLY,
        0,
    ))
    .unwrap();
    put(&p.memory.0, 0x5000, b"10 5 0\n");
    block_on(kernel.write(&p.caller(), fd, UserSlice::new(0x5000, 7))).unwrap();
    assert_eq!(exit(150, 9), 256);
    assert_eq!(
        exit(150, 10),
        320,
        "updated thresholds and zero interval apply"
    );
}
