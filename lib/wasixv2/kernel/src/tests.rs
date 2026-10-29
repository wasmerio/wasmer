//! The kernel driven from plain Rust, with no wasm engine involved.

use std::{
    cell::RefCell,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::executor::block_on;
use wasixv2_abi::{GuestPtr, GuestPtrMut};
use wasixv2_host::{ByteSink, ByteSource, Executor, LocalFuture};
use wasixv2_syscalls::{BINPRM_BUF_SIZE, Fault, SysErr, UserMemory, WASM_PAGE_SIZE};
use wasixv2_vfs_mem::MemFs;

use super::*;
use crate::sync::MutexExt;

#[derive(Default)]
struct FlatMemory(RefCell<Vec<u8>>);

impl UserMemory for FlatMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let memory = self.0.borrow();
        let range = addr as usize..addr as usize + buf.len();
        let result = memory
            .get(range)
            .map(|src| buf.copy_from_slice(src))
            .ok_or(Fault);
        Box::pin(async move { result })
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let mut memory = self.0.borrow_mut();
        let range = addr as usize..addr as usize + data.len();
        let result = memory
            .get_mut(range)
            .map(|dst| dst.copy_from_slice(data))
            .ok_or(Fault);
        Box::pin(async move { result })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        let size = self.0.borrow().len() as u64;
        Box::pin(async move { size })
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        let mut memory = self.0.borrow_mut();
        let previous = memory.len() as u64;
        memory.resize((previous + u64::from(pages) * WASM_PAGE_SIZE) as usize, 0);
        Box::pin(async move { Some(previous) })
    }
}

/// Fixed clocks, sleeps that finish at once and predictable "random" bytes.
pub(crate) struct TestHost;

impl Host for TestHost {
    fn realtime(&self) -> Duration {
        Duration::from_secs(1_700_000_000)
    }

    fn monotonic(&self) -> Duration {
        Duration::from_millis(1500)
    }

    fn sleep(&self, _: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async {})
    }

    // The clock never moves, so the default would wait forever.
    fn sleep_until(&self, _: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async {})
    }

    fn random(&self, buf: &mut [u8]) {
        buf.fill(0x5a);
    }

    fn executor(&self) -> &dyn Executor {
        self
    }
}

/// Blocking work runs inline; nothing here drives background futures.
impl Executor for TestHost {
    fn spawn_local(&self, _: LocalFuture<'static, ()>) {
        unimplemented!("kernel tests run no background host work")
    }

    fn spawn_blocking(&self, work: Box<dyn FnOnce() + Send>) -> LocalFuture<'static, ()> {
        Box::pin(async move { work() })
    }
}

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl ByteSink for Capture {
    fn write<'a>(&'a self, data: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        self.0.lock_or_recover().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

struct Input(Mutex<Vec<u8>>);

impl ByteSource for Input {
    fn poll_read(
        &self,
        _: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let mut input = self.0.lock_or_recover();
        let n = input.len().min(buf.len());
        buf[..n].copy_from_slice(&input[..n]);
        input.drain(..n);
        std::task::Poll::Ready(Ok(n))
    }
}

fn setup(input: &[u8]) -> (Kernel, TaskId, Arc<Capture>) {
    spawn(input, &[], &[])
}

fn spawn(input: &[u8], args: &[&str], env: &[&str]) -> (Kernel, TaskId, Arc<Capture>) {
    let stdin = Arc::new(Input(Mutex::new(input.to_vec())));
    spawn_with_stdin(stdin, args, env)
}

fn spawn_with_stdin(
    stdin: Arc<dyn ByteSource>,
    args: &[&str],
    env: &[&str],
) -> (Kernel, TaskId, Arc<Capture>) {
    spawn_on(test_kernel(), stdin, args, env)
}

/// A kernel on a memory root, with no init.
fn test_kernel() -> Kernel {
    let host: Arc<dyn Host> = Arc::new(TestHost);
    let clock = Arc::new(HostClock(host.clone()));
    let root = MemFs::new(clock.clone(), 0o755, 1, 1);
    let shm = MemFs::new(clock, 0o1777, 0, 0);
    Kernel::new(host, Arc::new(root)).with_shm(Arc::new(shm))
}

/// Spawns a process on `kernel`.
fn spawn_on(
    kernel: Kernel,
    stdin: Arc<dyn ByteSource>,
    args: &[&str],
    env: &[&str],
) -> (Kernel, TaskId, Arc<Capture>) {
    let stdout = Arc::new(Capture::default());
    let task = kernel.spawn(ProcessConfig {
        args: args.iter().map(|s| s.to_string()).collect(),
        env: env.iter().map(|s| s.to_string()).collect(),
        stdio: Stdio {
            stdin,
            stdout: stdout.clone(),
            stderr: Arc::new(Capture::default()),
        },
    });
    (kernel, task, stdout)
}

fn word(memory: &FlatMemory, addr: u32) -> u32 {
    let memory = memory.0.borrow();
    u32::from_le_bytes(memory[addr as usize..][..4].try_into().unwrap())
}

fn cstr_at(memory: &FlatMemory, addr: u32) -> String {
    let memory = memory.0.borrow();
    let bytes = &memory[addr as usize..];
    let len = bytes.iter().position(|&b| b == 0).unwrap();
    String::from_utf8(bytes[..len].to_vec()).unwrap()
}

#[test]
fn write_copies_from_guest_memory() {
    let (kernel, task, stdout) = setup(b"");
    let memory = FlatMemory(RefCell::new(b"..hello..".to_vec()));
    let caller = Caller::new(task, &memory);

    let n = block_on(kernel.write(&caller, Fd(1), UserSlice::new(2, 5)));

    assert_eq!(n, Ok(5));
    assert_eq!(*stdout.0.lock_or_recover(), b"hello");
}

#[test]
fn read_copies_out_only_what_arrived() {
    let (kernel, task, _) = setup(b"abc");
    let memory = FlatMemory(RefCell::new(vec![b'-'; 8]));
    let caller = Caller::new(task, &memory);

    let n = block_on(kernel.read(&caller, Fd(0), UserSliceMut::new(1, 6)));

    assert_eq!(n, Ok(3));
    assert_eq!(*memory.0.borrow(), b"-abc----");
}

#[test]
fn a_stdin_read_that_faults_leaves_the_input_for_the_next() {
    let (kernel, task, _) = setup(b"abc");
    let memory = FlatMemory(RefCell::new(vec![b'-'; 8]));
    let caller = Caller::new(task, &memory);

    let fault = block_on(kernel.read(&caller, Fd(0), UserSliceMut::new(6, 3)));
    let n = block_on(kernel.read(&caller, Fd(0), UserSliceMut::new(1, 6)));

    assert_eq!(fault, Err(SysErr::Errno(Errno::EFAULT)));
    assert_eq!(n, Ok(3));
    assert_eq!(*memory.0.borrow(), b"-abc----");
}

#[test]
fn bad_descriptor_wins_over_bad_pointer() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 8]));
    let caller = Caller::new(task, &memory);

    let bad_fd = block_on(kernel.write(&caller, Fd(-1), UserSlice::new(1 << 30, 4)));
    let bad_ptr = block_on(kernel.write(&caller, Fd(1), UserSlice::new(1 << 30, 4)));

    assert_eq!(bad_fd, Err(SysErr::Errno(Errno::EBADF)));
    assert_eq!(bad_ptr, Err(SysErr::Errno(Errno::EFAULT)));
}

#[test]
fn exit_group_leaves_pending_exit_with_low_byte() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(Vec::new()));
    let caller = Caller::new(task, &memory);

    assert_eq!(block_on(kernel.exit_group(&caller, 0x1_03)), Ok(()));

    assert_eq!(kernel.take_pending(task), Some(GuestAction::Exit(3)));
    assert_eq!(kernel.take_pending(task), None);
}

#[test]
fn startup_block_lays_out_argv_envp_and_auxv() {
    let (kernel, task, _) = spawn(b"", &["prog", "arg"], &["K=V"]);
    let memory = FlatMemory(RefCell::new(vec![0; 4096]));
    let caller = Caller::new(task, &memory);
    let base = 1024;

    let size = block_on(kernel.startup_size(&caller)).unwrap();
    assert_eq!(
        block_on(kernel.startup_get(&caller, UserSliceMut::new(base, size - 16))),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        block_on(kernel.startup_get(&caller, UserSliceMut::new(base, size))),
        Ok(0)
    );

    assert_eq!(size % 16, 0);
    assert_eq!(word(&memory, base), 2);
    assert_eq!(cstr_at(&memory, word(&memory, base + 4)), "prog");
    assert_eq!(cstr_at(&memory, word(&memory, base + 8)), "arg");
    assert_eq!(word(&memory, base + 12), 0);
    assert_eq!(cstr_at(&memory, word(&memory, base + 16)), "K=V");
    assert_eq!(word(&memory, base + 20), 0);
    let auxv: Vec<(u32, u32)> = (0..)
        .map(|i| {
            let at = base + 24 + i * 8;
            (word(&memory, at), word(&memory, at + 4))
        })
        .take_while(|&(key, _)| key != 0)
        .collect();
    assert!(auxv.contains(&(6, 65536)), "AT_PAGESZ: {auxv:?}");
    assert!(auxv.contains(&(11, 1)), "AT_UID: {auxv:?}");
    let random = auxv.iter().find(|(key, _)| *key == 25).unwrap().1;
    assert_eq!(memory.0.borrow()[random as usize..][..16], [0x5a; 16]);
    assert!(random + 16 <= base + size);
}

#[test]
fn startup_get_rejects_an_unaligned_buffer() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 4096]));
    let caller = Caller::new(task, &memory);

    let result = block_on(kernel.startup_get(&caller, UserSliceMut::new(8, 4096)));

    assert_eq!(result, Err(SysErr::Errno(Errno::EINVAL)));
}

#[test]
fn mmap_grows_memory_and_reuses_unmapped_pages_zeroed() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; WASM_PAGE_SIZE as usize]));
    let caller = Caller::new(task, &memory);
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let mmap = |len| block_on(kernel.mmap(&caller, 0, len, ProtFlags(3), anon, Fd(-1), 0));

    let first = mmap(100).unwrap();
    let second = mmap(70_000).unwrap();
    memory.0.borrow_mut()[first as usize] = 1;
    assert_eq!(block_on(kernel.munmap(&caller, first, 100)), Ok(0));
    let reused = mmap(10).unwrap();

    assert_eq!(first, WASM_PAGE_SIZE as u32);
    assert_eq!(second, 2 * WASM_PAGE_SIZE as u32);
    assert_eq!(block_on(caller.memory().size()), 4 * WASM_PAGE_SIZE);
    assert_eq!(reused, first);
    assert_eq!(memory.0.borrow()[first as usize], 0);
    let file_backed = block_on(kernel.mmap(
        &caller,
        0,
        10,
        ProtFlags(3),
        MapFlags::MAP_PRIVATE,
        Fd(0),
        0,
    ));
    assert_eq!(file_backed, Err(SysErr::Errno(Errno::ENODEV)));
    let bad_fd = block_on(kernel.mmap(
        &caller,
        0,
        10,
        ProtFlags(3),
        MapFlags::MAP_PRIVATE,
        Fd(3),
        0,
    ));
    assert_eq!(bad_fd, Err(SysErr::Errno(Errno::EBADF)));
    assert_eq!(mmap(0), Err(SysErr::Errno(Errno::EINVAL)));
    let unaligned = block_on(kernel.mmap(&caller, 0, 10, ProtFlags(3), anon, Fd(-1), 4096));
    assert_eq!(unaligned, Err(SysErr::Errno(Errno::EINVAL)));
}

/// A [`FlatMemory`] that drops pages on [`UserMemory::zero_pages`], as a Linux host does, and
/// counts the bytes the kernel writes and drops.
#[derive(Default)]
struct DroppingMemory {
    flat: FlatMemory,
    written: std::cell::Cell<u64>,
    dropped: std::cell::Cell<u64>,
}

impl UserMemory for DroppingMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.flat.read(addr, buf)
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.written.set(self.written.get() + data.len() as u64);
        self.flat.write(addr, data)
    }

    fn zero_pages(&self, addr: u32, len: u64) -> LocalFuture<'_, Result<(), Fault>> {
        let range = addr as usize..(u64::from(addr) + len) as usize;
        let result = match self.flat.0.borrow_mut().get_mut(range) {
            Some(pages) => {
                pages.fill(0);
                self.dropped.set(self.dropped.get() + len);
                Ok(())
            }
            None => Err(Fault),
        };
        Box::pin(async move { result })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        self.flat.size()
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        self.flat.grow(pages)
    }
}

/// libc-test's `t_vmfill`: probing for the largest mapping grows memory once, and every later
/// mapping reuses the probed pages. They are dropped, never written, so a `PROT_NONE` fill of
/// the address space takes no host memory.
#[test]
fn reused_pages_are_dropped_not_written() {
    let (kernel, task, _) = setup(b"");
    let memory = DroppingMemory::default();
    memory
        .flat
        .0
        .borrow_mut()
        .resize(WASM_PAGE_SIZE as usize, 0);
    let caller = Caller::new(task, &memory);
    let anon = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
    let page = WASM_PAGE_SIZE as u32;
    let mmap = |len, prot, flags, addr| {
        block_on(kernel.mmap(&caller, addr, len, prot, flags, Fd(-1), 0)).unwrap()
    };
    let munmap = |addr, len| assert_eq!(block_on(kernel.munmap(&caller, addr, len)), Ok(0));

    let probe = mmap(8 * page, ProtFlags(0), anon, 0);
    munmap(probe, 8 * page);
    assert_eq!(memory.dropped.get(), 0);
    let filled = mmap(6 * page, ProtFlags(0), anon, 0);
    assert_eq!(filled, probe);
    assert_eq!(memory.dropped.get(), 6 * WASM_PAGE_SIZE);

    // Pages the guest wrote before they were unmapped read zero once mapped again, whichever
    // way they are taken.
    let written = filled as usize + 2 * WASM_PAGE_SIZE as usize;
    memory.flat.0.borrow_mut()[written] = 7;
    munmap(filled, 6 * page);
    let fixed = MapFlags::MAP_FIXED | anon;
    assert_eq!(mmap(4 * page, ProtFlags(3), fixed, filled), filled);
    assert_eq!(memory.flat.0.borrow()[written], 0);
    memory.flat.0.borrow_mut()[written] = 7;
    assert_eq!(
        mmap(page, ProtFlags(3), fixed, filled + 2 * page),
        filled + 2 * page
    );
    assert_eq!(memory.flat.0.borrow()[written], 0);
    assert_eq!(memory.dropped.get(), 11 * WASM_PAGE_SIZE);
    assert_eq!(memory.written.get(), 0);
}

#[test]
fn sigprocmask_tracks_the_mask_but_never_blocks_sigkill() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 64]));
    let caller = Caller::new(task, &memory);
    // Blocks SIGINT (2) and SIGKILL (9).
    memory.0.borrow_mut()[8..16].copy_from_slice(&0x102u64.to_le_bytes());
    let sigprocmask = |how, set, old| {
        block_on(kernel.rt_sigprocmask(&caller, how, GuestPtr::new(set), GuestPtrMut::new(old), 8))
    };

    assert_eq!(sigprocmask(SigprocmaskHow::SIG_BLOCK, 8, 0), Ok(0));
    assert_eq!(sigprocmask(SigprocmaskHow::SIG_BLOCK, 0, 16), Ok(0));

    assert_eq!(memory.0.borrow()[16..24], 0x2u64.to_le_bytes());
    let bad_size = block_on(kernel.rt_sigprocmask(
        &caller,
        SigprocmaskHow::SIG_BLOCK,
        GuestPtr::NULL,
        GuestPtrMut::NULL,
        4,
    ));
    assert_eq!(bad_size, Err(SysErr::Errno(Errno::EINVAL)));
}

#[test]
fn signal_zero_probes_and_a_missing_target_comes_before_a_bad_signal() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 256]));
    let caller = Caller::new(task, &memory);
    // A siginfo from sigqueue(): si_code SI_QUEUE.
    memory.0.borrow_mut()[24..28].copy_from_slice(&wasixv2_abi::SiCode::SI_QUEUE.0.to_le_bytes());
    let me = task.0 as Pid;
    let sigqueue =
        |pid, sig| block_on(kernel.rt_sigqueueinfo(&caller, pid, Signal(sig), GuestPtr::new(16)));
    let kill = |pid, sig| block_on(kernel.sys_kill(&caller, pid, Signal(sig)));
    let err = |errno| Err(SysErr::Errno(errno));

    assert_eq!(sigqueue(me, 0), Ok(0));
    assert_eq!(sigqueue(999_999, 0), err(Errno::ESRCH));
    assert_eq!(sigqueue(999_999, 65), err(Errno::ESRCH));
    assert_eq!(sigqueue(me, 65), err(Errno::EINVAL));
    assert_eq!(kill(me, 0), Ok(0));
    assert_eq!(kill(999_999, 65), err(Errno::ESRCH));
    assert_eq!(kill(me, 65), err(Errno::EINVAL));
    // kill(-1) spares init, and the caller is the only other process.
    assert_eq!(kill(-1, 0), err(Errno::ESRCH));
}

#[test]
fn sa_resethand_resets_only_the_handler() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 64]));
    let caller = Caller::new(task, &memory);
    let flags = wasixv2_abi::SaFlags::SA_RESETHAND.0 | wasixv2_abi::SaFlags::SA_SIGINFO.0;
    {
        let mut memory = memory.0.borrow_mut();
        memory[8..12].copy_from_slice(&7u32.to_le_bytes());
        memory[12..16].copy_from_slice(&flags.to_le_bytes());
    }
    let sigaction = |act, oact| {
        block_on(kernel.rt_sigaction(
            &caller,
            Signal::SIGUSR1,
            GuestPtr::new(act),
            GuestPtrMut::new(oact),
            8,
        ))
    };
    assert_eq!(block_on(kernel.signal_callback_register(&caller, 3)), Ok(0));
    assert_eq!(sigaction(8, 0), Ok(0));
    assert_eq!(
        block_on(kernel.sys_kill(&caller, task.0 as Pid, Signal::SIGUSR1)),
        Ok(0)
    );
    assert!(matches!(
        kernel.take_pending(task),
        Some(GuestAction::Signal(wasixv2_syscalls::SignalFrame {
            handler: 7,
            ..
        }))
    ));

    assert_eq!(sigaction(0, 32), Ok(0));
    let memory = memory.0.borrow();
    assert_eq!(memory[32..36], 0u32.to_le_bytes());
    assert_eq!(memory[36..40], flags.to_le_bytes());
}

#[test]
fn files_round_trip_through_the_vfs() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 256]));
    let caller = Caller::new(task, &memory);
    memory.0.borrow_mut()[..7].copy_from_slice(b"/f.txt\0");
    memory.0.borrow_mut()[32..37].copy_from_slice(b"hello");
    let open = |flags| block_on(kernel.openat(&caller, Fd(-100), GuestPtr::new(0), flags, 0o644));

    let fd = open(OpenFlags::O_CREAT | OpenFlags::O_RDWR | OpenFlags::O_APPEND).unwrap();
    assert_eq!(
        block_on(kernel.write(&caller, fd, UserSlice::new(32, 5))),
        Ok(5)
    );
    assert_eq!(
        block_on(kernel.write(&caller, fd, UserSlice::new(32, 2))),
        Ok(2)
    );
    let read = block_on(kernel.pread64(&caller, fd, UserSliceMut::new(64, 16), 0));

    assert_eq!(fd, Fd(3));
    assert_eq!(read, Ok(7));
    assert_eq!(&memory.0.borrow()[64..71], b"hellohe");
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    assert_eq!(
        block_on(kernel.close(&caller, fd)),
        Err(SysErr::Errno(Errno::EBADF))
    );
    let exclusive = open(OpenFlags::O_CREAT | OpenFlags::O_EXCL | OpenFlags::O_RDWR);
    assert_eq!(exclusive, Err(SysErr::Errno(Errno::EEXIST)));
    let unlinked = block_on(kernel.unlinkat(&caller, Fd(-100), GuestPtr::new(0), AtFlags(0)));
    assert_eq!(unlinked, Ok(0));
    assert_eq!(open(OpenFlags::O_RDONLY), Err(SysErr::Errno(Errno::ENOENT)));
}

#[test]
fn uname_reports_the_linux_release_of_the_headers_and_the_wasixv2_abi_version() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);

    assert_eq!(block_on(kernel.uname(&caller, GuestPtrMut::new(8))), Ok(0));
    assert_eq!(block_on(kernel.getuid(&caller)), Ok(1));

    let field = |i: u32| cstr_at(&memory, 8 + i * 65);
    assert_eq!(field(0), "wasixv2");
    assert_eq!(field(2), "7.2.0");
    assert_eq!(field(3), "#1 SMP wasixv2 2.0.0");
    assert_eq!(field(4), "wasm32");
}

fn put(memory: &FlatMemory, addr: u32, bytes: &[u8]) {
    memory.0.borrow_mut()[addr as usize..][..bytes.len()].copy_from_slice(bytes);
}

#[test]
fn chdir_mkdir_and_getcwd_follow_the_working_directory() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);
    put(&memory, 0, b"a\0");
    put(&memory, 8, b"b\0");
    put(&memory, 16, b"..\0");
    let cstr = GuestPtr::new;

    assert_eq!(
        block_on(kernel.mkdirat(&caller, Fd(-100), cstr(0), 0o755)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.chdir(&caller, cstr(0))), Ok(0));
    assert_eq!(
        block_on(kernel.mkdirat(&caller, Fd(-100), cstr(8), 0o755)),
        Ok(0)
    );
    assert_eq!(block_on(kernel.chdir(&caller, cstr(8))), Ok(0));
    let len = block_on(kernel.getcwd(&caller, UserSliceMut::new(100, 64)));
    assert_eq!(len, Ok(5));
    assert_eq!(cstr_at(&memory, 100), "/a/b");
    assert_eq!(
        block_on(kernel.getcwd(&caller, UserSliceMut::new(100, 4))),
        Err(SysErr::Errno(Errno::ERANGE))
    );
    // Removing the working directory leaves getcwd with nothing to report.
    assert_eq!(block_on(kernel.chdir(&caller, cstr(16))), Ok(0));
    assert_eq!(
        block_on(kernel.unlinkat(&caller, Fd(-100), cstr(8), AtFlags::AT_REMOVEDIR)),
        Ok(0)
    );
    assert_eq!(
        block_on(kernel.mkdirat(&caller, Fd(-100), cstr(8), 0o755)),
        Ok(0)
    );
}

#[test]
fn getdents_lists_dots_then_entries_and_resumes() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 1024]));
    let caller = Caller::new(task, &memory);
    put(&memory, 0, b"/\0");
    put(&memory, 8, b"/x\0");
    block_on(kernel.mkdirat(&caller, Fd(-100), GuestPtr::new(8), 0o755)).unwrap();
    let dir = block_on(kernel.openat(
        &caller,
        Fd(-100),
        GuestPtr::new(0),
        OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY,
        0,
    ))
    .unwrap();

    let mut names = Vec::new();
    loop {
        let n = block_on(kernel.getdents64(&caller, dir, GuestPtrMut::new(256), 40)).unwrap();
        if n == 0 {
            break;
        }
        let reclen = u16::from_le_bytes([memory.0.borrow()[256 + 16], memory.0.borrow()[257 + 16]]);
        assert_eq!(n as u16, reclen, "a 40-byte buffer holds one record");
        names.push(cstr_at(&memory, 256 + 19));
    }

    assert_eq!(names, [".", "..", "x"]);
    assert_eq!(
        block_on(kernel.getdents64(&caller, dir, GuestPtrMut::new(256), 8)),
        Ok(0)
    );
}

#[test]
fn posix_locks_go_away_when_any_descriptor_closes() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);
    put(&memory, 0, b"/l\0");
    let open = || {
        block_on(kernel.openat(
            &caller,
            Fd(-100),
            GuestPtr::new(0),
            OpenFlags::O_CREAT | OpenFlags::O_RDWR,
            0o600,
        ))
        .unwrap()
    };
    let (first, second) = (open(), open());
    // flock64 { l_type = F_WRLCK, l_whence = SEEK_SET, l_start = 0, l_len = 0 } at 64.
    put(&memory, 64, &[1, 0, 0, 0]);
    let set = |fd| block_on(kernel.fcntl64(&caller, fd, FcntlCmd::F_SETLK64, 64));
    assert_eq!(set(first), Ok(0));
    // The same process may relock through another descriptor.
    assert_eq!(set(second), Ok(0));

    assert_eq!(block_on(kernel.close(&caller, second)), Ok(0));
    // An OFD lock from `first` now finds nothing in its way.
    assert_eq!(
        block_on(kernel.fcntl64(&caller, first, FcntlCmd::F_OFD_SETLK, 64)),
        Ok(0)
    );
}

#[test]
fn a_blocked_flock_or_ofd_lock_ends_on_a_signal() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);
    put(&memory, 0, b"/l\0");
    let open = || {
        block_on(kernel.openat(
            &caller,
            Fd(-100),
            GuestPtr::new(0),
            OpenFlags::O_CREAT | OpenFlags::O_RDWR,
            0o600,
        ))
        .unwrap()
    };
    let (first, second) = (open(), open());
    let exclusive = FlockOp::LOCK_EX;
    assert_eq!(block_on(kernel.flock(&caller, first, exclusive)), Ok(0));
    // An OFD write lock over the whole file from `first`, as flock64 at 64.
    put(&memory, 64, &[1, 0, 0, 0]);
    let ofd = |fd, cmd| block_on(kernel.fcntl64(&caller, fd, cmd, 64));
    assert_eq!(ofd(first, FcntlCmd::F_OFD_SETLK), Ok(0));

    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            kernel.kill(task, Signal::SIGUSR1).unwrap();
        });
        // Each waits for `first`'s lock until the signal ends the wait.
        assert_eq!(
            block_on(kernel.flock(&caller, second, exclusive)),
            Err(SysErr::Restart)
        );
    });
    assert_eq!(ofd(second, FcntlCmd::F_OFD_SETLKW), Err(SysErr::Restart));
    // A lock that is free is still taken with the signal pending.
    let unlock = FlockOp::LOCK_UN;
    assert_eq!(block_on(kernel.flock(&caller, first, unlock)), Ok(0));
    assert_eq!(block_on(kernel.flock(&caller, second, exclusive)), Ok(0));
}

#[test]
fn file_io_skips_notification_while_nothing_is_watched() {
    use wasixv2_abi::{InotifyInitFlags, InotifyMask};
    use wasixv2_vfs::FsWatcher;

    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);
    put(&memory, 0, b"/\0");
    assert!(!kernel.inotify.watching());

    let fd = block_on(kernel.inotify_init1(&caller, InotifyInitFlags(0))).unwrap();
    let add = kernel.inotify_add_watch(&caller, fd, GuestPtr::new(0), InotifyMask::IN_MODIFY);
    let wd = block_on(add).unwrap();
    assert!(kernel.inotify.watching());
    assert_eq!(block_on(kernel.inotify_rm_watch(&caller, fd, wd)), Ok(0));
    assert!(!kernel.inotify.watching());

    // Closing the instance drops its watches too.
    let add = kernel.inotify_add_watch(&caller, fd, GuestPtr::new(0), InotifyMask::IN_MODIFY);
    block_on(add).unwrap();
    assert_eq!(block_on(kernel.close(&caller, fd)), Ok(0));
    assert!(!kernel.inotify.watching());
}

/// Sets `pid`'s soft `RLIMIT_NOFILE` to `limit`, with a hard limit of 64, from `caller`.
fn set_nofile(kernel: &Kernel, caller: &Caller<'_>, memory: &FlatMemory, pid: i32, limit: u64) {
    put(memory, 256, &limit.to_le_bytes());
    put(memory, 264, &64u64.to_le_bytes());
    let nofile = wasixv2_abi::RlimitResource::RLIMIT_NOFILE;
    let set = kernel.prlimit64(caller, pid, nofile, GuestPtr::new(256), GuestPtrMut::NULL);
    assert_eq!(block_on(set), Ok(0));
}

#[test]
fn an_open_at_the_descriptor_limit_creates_nothing() {
    use ipc::errno;
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);
    put(&memory, 0, b"/new\0");
    // Descriptors 0 to 2 are stdio, so there is no room left.
    set_nofile(&kernel, &caller, &memory, 0, 3);
    let create = OpenFlags::O_CREAT | OpenFlags::O_RDWR | OpenFlags::O_TRUNC;
    let open = |flags| block_on(kernel.openat(&caller, Fd(-100), GuestPtr::new(0), flags, 0o600));
    assert_eq!(errno(open(create)), Errno::EMFILE);
    set_nofile(&kernel, &caller, &memory, 0, 8);
    assert_eq!(errno(open(OpenFlags::O_RDONLY)), Errno::ENOENT);

    // A pipe whose descriptors cannot be copied out installs none of them.
    let fault = wasixv2_abi::FdPair::new(1 << 30);
    let pipe = block_on(kernel.pipe2(&caller, fault, OpenFlags(0)));
    assert_eq!(errno(pipe), Errno::EFAULT);
    assert_eq!(open(create), Ok(Fd(3)));
}

#[test]
fn prlimit_of_another_process_sets_its_descriptor_limit() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);
    let other = kernel.spawn(ProcessConfig {
        args: vec!["other".into()],
        env: Vec::new(),
        stdio: Stdio::from_fns(|_| {}, |_| {}),
    });
    set_nofile(&kernel, &caller, &memory, other.0 as i32, 3);
    let dup = |task| {
        let stdin = kernel.file(task, Fd(0)).unwrap();
        kernel.files(task).lock_or_recover().install(stdin, false)
    };
    assert_eq!(dup(other), None);
    assert_eq!(dup(task), Some(Fd(3)));
}

mod access;
mod cred;
mod fs;
mod futex;
mod ipc;
mod jobctl;
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
mod mapped;
mod mapping;
mod memfd;
mod mount;
mod net;
mod ns;
mod proc;
mod procfs;
mod ptimer;
mod pty;
mod sched;
mod sctp;
mod signal;
mod stdio;
mod sysvipc;
mod task;
mod timerfd;
mod tty;
