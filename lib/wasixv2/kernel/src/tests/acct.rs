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
