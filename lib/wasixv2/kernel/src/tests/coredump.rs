//! Core dumps (`fs/coredump.c`): `core_pattern`, `RLIMIT_CORE`, `coredump_filter`, the
//! `WCOREDUMP` status, the pipe helper and what pidfds and accounting tell of a dump.

use wasixv2_abi::{
    Idtype, PidfdInfo, PidfdInfoMask, PrctlOption, Rlimit64, RlimitResource, SiCode, Siginfo,
    SiginfoPtr, WaitOptions,
};
use wasixv2_syscalls::{read_value, write_value};
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
            memory: ForkableMemory(FlatMemory(RefCell::new(vec![0x5a; 65536]))),
        }
    }

    fn caller(&self) -> Caller<'_> {
        Caller::new(self.task, &self.memory)
    }

    fn path(&self, at: &str) -> Cstr {
        put(&self.memory.0, 0x1000, format!("{at}\0").as_bytes());
        GuestPtr::new(0x1000)
    }

    fn open(&self, kernel: &Kernel, at: &str, flags: OpenFlags) -> SysResult<Fd> {
        block_on(kernel.openat(&self.caller(), CWD, self.path(at), flags, 0o644))
    }

    fn write(&self, kernel: &Kernel, at: &str, text: &str) {
        let fd = self.open(kernel, at, OpenFlags::O_WRONLY).unwrap();
        put(&self.memory.0, 0x2000, text.as_bytes());
        let buf = UserSlice::new(0x2000, text.len() as u32);
        block_on(kernel.write(&self.caller(), fd, buf)).unwrap();
        block_on(kernel.close(&self.caller(), fd)).unwrap();
    }

    fn read(&self, kernel: &Kernel, at: &str) -> SysResult<Vec<u8>> {
        let fd = self.open(kernel, at, OpenFlags::O_RDONLY)?;
        let mut out = Vec::new();
        loop {
            let n = block_on(kernel.read(&self.caller(), fd, UserSliceMut::new(0x4000, 0x4000)))?;
            if n == 0 {
                break;
            }
            out.extend(get(&self.memory.0, 0x4000, n as usize));
        }
        block_on(kernel.close(&self.caller(), fd)).unwrap();
        Ok(out)
    }

    fn set_core_limit(&self, kernel: &Kernel, limit: u64) {
        let rlimit = Rlimit64 {
            rlim_cur: limit,
            rlim_max: u64::MAX,
        };
        block_on(write_value(&self.memory, GuestPtrMut::new(0x3000), &rlimit)).unwrap();
        let caller = self.caller();
        let set = kernel.prlimit64(
            &caller,
            0,
            RlimitResource::RLIMIT_CORE,
            GuestPtr::new(0x3000),
            GuestPtrMut::NULL,
        );
        assert_eq!(block_on(set), Ok(0));
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

    /// The process dies of `sig`, which it takes on its next return to the guest, leaving its
    /// memory as its execution layer would.
    fn die_of(&self, kernel: &Kernel, pid: Pid, sig: Signal) {
        let caller = self.caller();
        assert_eq!(block_on(kernel.sys_kill(&caller, pid, sig)), Ok(0));
        assert!(matches!(
            kernel.take_pending(self.task),
            Some(GuestAction::Exit(_))
        ));
        block_on(kernel.exit_mm(&caller));
        kernel.release(self.task);
    }

    /// `waitid` for `pid`: its `si_code` and `si_status`.
    fn reap(&self, kernel: &Kernel, pid: Pid) -> (i32, i32) {
        let options = WaitOptions(WaitOptions::WEXITED.0);
        let caller = self.caller();
        let info = SiginfoPtr::new(0x300);
        let wait = kernel.waitid(&caller, Idtype::P_PID, pid, info, options, RusagePtr::NULL);
        assert_eq!(block_on(wait), Ok(0));
        let info: Siginfo = block_on(read_value(&self.memory, GuestPtr::new(0x300))).unwrap();
        let status = i32::from_le_bytes(info.sifields[8..12].try_into().unwrap());
        (info.si_code, status)
    }
}

fn machine() -> (Kernel, P) {
    let (kernel, task, _) = super::cred::machine_with(|kernel| {
        kernel.with_credentials(Credentials::new(0, 0)).with_init()
    });
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/proc", kernel.procfs(), MountFlags::default(), true)).unwrap();
    (kernel, P::new(task))
}

/// The program header of the linear memory: offset, file size, memory size.
fn memory_segment(core: &[u8]) -> (u32, u32, u32) {
    let word = |at: usize| u32::from_le_bytes(core[at..at + 4].try_into().unwrap());
    let phdr = 52 + 32;
    assert_eq!(word(phdr), 1, "PT_LOAD");
    (word(phdr + 4), word(phdr + 16), word(phdr + 20))
}

/// A signal that dumps core writes the dying process's memory to the file `core_pattern`
/// names, as an ELF core file, and the parent sees `CLD_DUMPED`; `RLIMIT_CORE` 0, the
/// default, dumps nothing.
#[test]
fn a_core_signal_dumps_the_memory_to_the_pattern_file() {
    let (kernel, p) = machine();
    let caller = p.caller();
    let mkdir = kernel.mkdirat(&caller, CWD, p.path("/cores"), 0o755);
    block_on(mkdir).unwrap();
    p.write(
        &kernel,
        "/proc/sys/kernel/core_pattern",
        "/cores/core.%p.%e.%s\n",
    );
    assert_eq!(
        p.read(&kernel, "/proc/sys/kernel/core_pattern"),
        Ok(b"/cores/core.%p.%e.%s\n".to_vec())
    );

    let (child, pid) = p.fork(&kernel);
    child.die_of(&kernel, pid, Signal::SIGQUIT);
    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_KILLED.0, Signal::SIGQUIT.0)
    );
    assert_eq!(
        errno(p.read(&kernel, &format!("/cores/core.{pid}.test.3"))),
        Errno::ENOENT
    );

    p.set_core_limit(&kernel, u64::MAX);
    let (child, pid) = p.fork(&kernel);
    child.die_of(&kernel, pid, Signal::SIGABRT);
    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_DUMPED.0, Signal::SIGABRT.0)
    );
    let core = p
        .read(&kernel, &format!("/cores/core.{pid}.test.6"))
        .unwrap();
    assert_eq!(&core[..4], b"\x7fELF");
    let (offset, filesz, memsz) = memory_segment(&core);
    assert_eq!((filesz, memsz), (65536, 65536));
    assert_eq!(core.len(), (offset + filesz) as usize);
    assert!(core[offset as usize..].iter().all(|&b| b == 0x5a));

    // A signal that only terminates dumps nothing.
    let (child, pid) = p.fork(&kernel);
    child.die_of(&kernel, pid, Signal::SIGTERM);
    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_KILLED.0, Signal::SIGTERM.0)
    );
}

/// A trap's signal (`force_sig_fault`), which the execution layer hands the kernel before the
/// task's `exit_mm`, dumps core there as a signal taken on the way to the guest does.
#[test]
fn a_trap_dumps_core() {
    let (kernel, p) = machine();
    let caller = p.caller();
    block_on(kernel.mkdirat(&caller, CWD, p.path("/cores"), 0o755)).unwrap();
    p.write(
        &kernel,
        "/proc/sys/kernel/core_pattern",
        "/cores/core.%p.%s",
    );
    p.set_core_limit(&kernel, u64::MAX);

    let (child, pid) = p.fork(&kernel);
    assert!(kernel.fault(child.task, Signal::SIGILL.0));
    assert_eq!(
        kernel.take_pending(child.task),
        Some(GuestAction::Exit(128 + Signal::SIGILL.0))
    );
    block_on(kernel.exit_mm(&child.caller()));
    kernel.release(child.task);

    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_DUMPED.0, Signal::SIGILL.0)
    );
    let core = p.read(&kernel, &format!("/cores/core.{pid}.4")).unwrap();
    assert_eq!(&core[..4], b"\x7fELF");
}

/// `core_uses_pid` names a pattern without `%p` after the process; `coredump_filter` without
/// private anonymous memory leaves the memory out; a limit cuts the core short.
#[test]
fn the_name_filter_and_limit_shape_the_core() {
    let (kernel, p) = machine();
    p.set_core_limit(&kernel, u64::MAX);
    p.write(&kernel, "/proc/sys/kernel/core_uses_pid", "1");
    assert_eq!(
        p.read(&kernel, "/proc/self/coredump_filter"),
        Ok(b"00000033\n".to_vec())
    );
    p.write(&kernel, "/proc/self/coredump_filter", "0x32");

    let (child, pid) = p.fork(&kernel);
    assert_eq!(
        child.read(&kernel, "/proc/self/coredump_filter"),
        Ok(b"00000032\n".to_vec())
    );
    child.die_of(&kernel, pid, Signal::SIGSEGV);
    p.reap(&kernel, pid);
    let core = p.read(&kernel, &format!("/core.{pid}")).unwrap();
    let (offset, filesz, memsz) = memory_segment(&core);
    assert_eq!((filesz, memsz), (0, 65536));
    assert_eq!(core.len(), offset as usize);

    p.write(&kernel, "/proc/self/coredump_filter", "0x33");
    p.set_core_limit(&kernel, 8192);
    let (child, pid) = p.fork(&kernel);
    child.die_of(&kernel, pid, Signal::SIGSEGV);
    // A core that started counts, cut short or not (`COREDUMP_STATE_STARTED`).
    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_DUMPED.0, Signal::SIGSEGV.0)
    );
    let core = p.read(&kernel, &format!("/core.{pid}")).unwrap();
    assert!(core.len() <= 8192, "{}", core.len());
}

/// A process that is not dumpable, and a pipe with no helper to start, dump nothing; the
/// latter is logged.
#[test]
fn some_processes_and_patterns_dump_nothing() {
    let (kernel, p) = machine();
    p.set_core_limit(&kernel, u64::MAX);
    let (child, pid) = p.fork(&kernel);
    let caller = child.caller();
    let off = kernel.prctl(&caller, PrctlOption::PR_SET_DUMPABLE, 0, 0, 0, 0);
    assert_eq!(block_on(off), Ok(0));
    child.die_of(&kernel, pid, Signal::SIGABRT);
    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_KILLED.0, Signal::SIGABRT.0)
    );
    assert_eq!(errno(p.read(&kernel, "/core")), Errno::ENOENT);

    p.write(&kernel, "/proc/sys/kernel/core_pattern", "|/bin/catch %p");
    let (child, pid) = p.fork(&kernel);
    child.die_of(&kernel, pid, Signal::SIGABRT);
    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_KILLED.0, Signal::SIGABRT.0)
    );
    let log = block_on(kernel.syslog(&p.caller(), 3, GuestPtrMut::new(0x4000), 4096)).unwrap();
    let log = String::from_utf8(get(&p.memory.0, 0x4000, log as usize)).unwrap();
    assert!(
        log.contains("(test): Core dump to |/bin/catch disabled\n"),
        "{log}"
    );
}

/// The helper a pattern pipes to.
#[derive(Default)]
struct Helper(Mutex<Vec<TaskId>>);

impl UsermodeHelper for Helper {
    fn start(&self, task: TaskId, _image: wasixv2_syscalls::Image) {
        self.0.lock_or_recover().push(task);
    }
}

/// A pipe pattern starts its helper as root, with the expanded arguments and the core on its
/// standard input; the pidfd of the dumped process tells of the dump.
#[test]
fn a_pipe_pattern_feeds_the_core_to_its_helper() {
    let (kernel, p) = machine();
    let helper = Arc::new(Helper::default());
    kernel.set_usermode_helper(helper.clone());
    let caller = p.caller();
    let mkdir = kernel.mkdirat(&caller, CWD, p.path("/bin"), 0o755);
    block_on(mkdir).unwrap();
    let fd = p
        .open(
            &kernel,
            "/bin/catch",
            OpenFlags::O_CREAT | OpenFlags::O_WRONLY,
        )
        .unwrap();
    put(&p.memory.0, 0x2000, b"\0asm\x01\0\0\0");
    block_on(kernel.write(&p.caller(), fd, UserSlice::new(0x2000, 8))).unwrap();
    let caller = p.caller();
    let chmod = kernel.fchmodat(&caller, CWD, p.path("/bin/catch"), 0o755);
    block_on(chmod).unwrap();
    p.write(
        &kernel,
        "/proc/sys/kernel/core_pattern",
        "|/bin/catch %p %s",
    );
    p.write(&kernel, "/proc/self/coredump_filter", "0");

    let (child, pid) = p.fork(&kernel);
    let pidfd = block_on(kernel.pidfd_open(&p.caller(), pid, 0)).unwrap();
    child.die_of(&kernel, pid, Signal::SIGABRT);
    let helpers = helper.0.lock_or_recover().clone();
    assert_eq!(helpers.len(), 1);
    let catch = P::new(helpers[0]);
    assert_eq!(
        catch.read(&kernel, "/proc/self/cmdline"),
        Ok(format!("/bin/catch\0{pid}\0{}\0", Signal::SIGABRT.0).into_bytes())
    );
    let status = catch.read(&kernel, "/proc/self/status").unwrap();
    assert!(
        String::from_utf8(status)
            .unwrap()
            .contains("\nUid:\t0\t0\t0\t0\n")
    );
    let n = block_on(kernel.read(&catch.caller(), Fd(0), UserSliceMut::new(0x4000, 0x4000)));
    assert!(n.unwrap() > 52);
    assert_eq!(get(&catch.memory.0, 0x4000, 4), b"\x7fELF");
    assert_eq!(
        p.reap(&kernel, pid),
        (SiCode::CLD_DUMPED.0, Signal::SIGABRT.0)
    );

    let mask = PidfdInfoMask::PIDFD_INFO_COREDUMP.0 | PidfdInfoMask::PIDFD_INFO_EXIT.0;
    put(&p.memory.0, 0x500, &mask.to_le_bytes());
    let request = IoctlRequest::PIDFD_GET_INFO;
    block_on(kernel.ioctl(&p.caller(), pidfd, request, 0x500)).unwrap();
    let info: PidfdInfo = block_on(read_value(&p.memory, GuestPtr::new(0x500))).unwrap();
    assert_eq!(
        (info.coredump_mask, info.coredump_signal),
        (1 | 4, Signal::SIGABRT.0 as u32)
    );
    assert_eq!(info.exit_code, Signal::SIGABRT.0 | 0x80);
}

fn note_description(core: &[u8], kind: u32) -> Vec<u8> {
    let word = |at: usize| u32::from_le_bytes(core[at..at + 4].try_into().unwrap()) as usize;
    let mut at = word(52 + 4);
    let end = at + word(52 + 16);
    while at < end {
        let name_size = word(at);
        let desc_size = word(at + 4);
        let ty = word(at + 8) as u32;
        let desc = at + 12 + name_size.next_multiple_of(4);
        if ty == kind {
            assert_eq!(&core[at + 12..at + 12 + name_size], b"CORE\0");
            return core[desc..desc + desc_size].to_vec();
        }
        at = desc + desc_size.next_multiple_of(4);
    }
    panic!("missing ELF note {kind:#x}")
}

#[test]
fn coredump_notes_include_saved_auxv_and_file_vma_offsets_and_names() {
    use wasixv2_abi::{MapFlags, ProtFlags};
    let (kernel, p) = machine();
    p.set_core_limit(&kernel, u64::MAX);
    let (child, pid) = p.fork(&kernel);
    let caller = child.caller();
    let page = crate::mm::PAGE_SIZE as u32;
    let auxv = [6u32, page, 11, 123, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let mut saved = auxv.clone();
    saved.extend([0x99; 16]);
    *kernel.process(child.task).auxv.lock_or_recover() = saved;
    let fd = child
        .open(
            &kernel,
            "/mapped file",
            OpenFlags::O_CREAT | OpenFlags::O_RDWR,
        )
        .unwrap();
    block_on(kernel.ftruncate64(&caller, fd, 4 * i64::from(page))).unwrap();
    let private = block_on(kernel.mmap(
        &caller,
        0,
        2 * page,
        ProtFlags::PROT_READ,
        MapFlags::MAP_PRIVATE,
        fd,
        i64::from(page),
    ))
    .unwrap();
    block_on(kernel.mprotect(
        &caller,
        private + page,
        page,
        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
    ))
    .unwrap();
    let shared = block_on(kernel.mmap(
        &caller,
        0,
        page,
        ProtFlags::PROT_READ,
        MapFlags::MAP_SHARED,
        fd,
        0,
    ))
    .unwrap();
    child.die_of(&kernel, pid, Signal::SIGABRT);
    let core = p.read(&kernel, "core").unwrap();
    assert_eq!(note_description(&core, 6), auxv, "NT_AUXV stops at AT_NULL");
    let files = note_description(&core, 0x46494c45);
    let words = |at: usize| u32::from_le_bytes(files[at..at + 4].try_into().unwrap());
    assert_eq!((words(0), words(4)), (3, page));
    assert_eq!(
        (words(8), words(12), words(16)),
        (private, private + page, 1)
    );
    assert_eq!(
        (words(20), words(24), words(28)),
        (private + page, private + 2 * page, 2)
    );
    assert_eq!(
        (words(32), words(36), words(40)),
        (shared, shared + page, 0)
    );
    assert_eq!(&files[44..], b"/mapped file\0/mapped file\0/mapped file\0");
}
