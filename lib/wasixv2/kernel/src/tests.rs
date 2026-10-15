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
use wasixv2_syscalls::{Fault, SysErr, UserMemory, WASM_PAGE_SIZE};
use wasixv2_vfs_mem::MemDir;

use super::*;

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
struct TestHost;

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
        self.0.lock().unwrap().extend_from_slice(data);
        Box::pin(async move { Ok(data.len()) })
    }
}

struct Input(Mutex<Vec<u8>>);

impl ByteSource for Input {
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> LocalFuture<'a, io::Result<usize>> {
        let mut input = self.0.lock().unwrap();
        let n = input.len().min(buf.len());
        buf[..n].copy_from_slice(&input[..n]);
        input.drain(..n);
        Box::pin(async move { Ok(n) })
    }
}

fn setup(input: &[u8]) -> (Kernel, TaskId, Arc<Capture>) {
    spawn(input, &[], &[])
}

fn spawn(input: &[u8], args: &[&str], env: &[&str]) -> (Kernel, TaskId, Arc<Capture>) {
    let kernel = Kernel::new(Arc::new(TestHost), MemDir::root());
    let stdout = Arc::new(Capture::default());
    let task = kernel.spawn(ProcessConfig {
        args: args.iter().map(|s| s.to_string()).collect(),
        env: env.iter().map(|s| s.to_string()).collect(),
        stdio: Stdio {
            stdin: Arc::new(Input(Mutex::new(input.to_vec()))),
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
    assert_eq!(*stdout.0.lock().unwrap(), b"hello");
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
        Fd(3),
        0,
    ));
    assert_eq!(file_backed, Err(SysErr::Errno(Errno::ENODEV)));
    assert_eq!(mmap(0), Err(SysErr::Errno(Errno::EINVAL)));
    let unaligned = block_on(kernel.mmap(&caller, 0, 10, ProtFlags(3), anon, Fd(-1), 4096));
    assert_eq!(unaligned, Err(SysErr::Errno(Errno::EINVAL)));
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
fn uname_reports_the_wasixv2_abi_version() {
    let (kernel, task, _) = setup(b"");
    let memory = FlatMemory(RefCell::new(vec![0; 512]));
    let caller = Caller::new(task, &memory);

    assert_eq!(block_on(kernel.uname(&caller, GuestPtrMut::new(8))), Ok(0));
    assert_eq!(block_on(kernel.getuid(&caller)), Ok(1));

    let field = |i: u32| cstr_at(&memory, 8 + i * 65);
    assert_eq!(field(0), "wasixv2");
    assert_eq!(field(2), "2.0.0");
    assert_eq!(field(4), "wasm32");
}

mod ipc;
