//! Shared objects whose pages the memory shows itself: `MAP_SHARED | MAP_ANONYMOUS` memory,
//! SysV segments and memfd files on a host that has shared memory, over memory that can place
//! an object's pages, as the execution layer's on Unix.

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    sync::atomic::{AtomicU32, Ordering},
};

use futures::FutureExt as _;
use wasixv2_abi::{
    Cstr, FcntlCmd, FutexOp, IpcCmd, MadviseAdvice, MapFlags, MemfdFlags, MremapFlags, MsyncFlags,
    ProtFlags, SealFlags, Shmid64Ds,
};
use wasixv2_pages::{Memfd, Provider, SharedPages, map_at, map_private_at};
use wasixv2_syscalls::{AtomicWait, AtomicWord, read_value};
use wasixv2_vfs::MountFlags;

use super::{ipc::errno, *};

const PAGE: u32 = WASM_PAGE_SIZE as u32;
const RW: ProtFlags = ProtFlags(3);
/// Pages a [`MappedMemory`] may grow to.
const RESERVED: usize = 64;

/// Linear memory the way the execution layer has it on Unix: one host mapping reserved for all
/// it may grow to, whose ranges can show a shared object's pages, read-only ones without write
/// access, where writes fault.
pub(super) struct MappedMemory {
    base: *mut u8,
    size: Cell<usize>,
    forkable: bool,
    read_only: RefCell<Vec<Range<u32>>>,
}

impl MappedMemory {
    /// `pages` pages of zeroed memory; with `forkable`, one an engine can duplicate.
    pub(super) fn new(pages: usize, forkable: bool) -> Self {
        let len = RESERVED * PAGE as usize;
        let prot = libc::PROT_READ | libc::PROT_WRITE;
        let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS;
        // SAFETY: a fresh mapping, owned by the returned value.
        let base = unsafe { libc::mmap(std::ptr::null_mut(), len, prot, flags, -1, 0) };
        assert_ne!(base, libc::MAP_FAILED);
        Self {
            base: base.cast(),
            size: Cell::new(pages * PAGE as usize),
            forkable,
            read_only: RefCell::new(Vec::new()),
        }
    }

    /// A fork child's memory: a copy of this one, which shows `shared` again as the execution
    /// layer's copy does.
    pub(super) fn fork(&self, shared: &[wasixv2_syscalls::SharedMapping]) -> Self {
        let child = Self::new(self.size.get() / PAGE as usize, self.forkable);
        // SAFETY: both ranges lie inside their reservations, which do not overlap.
        unsafe { std::ptr::copy_nonoverlapping(self.base, child.base, self.size.get()) };
        for m in shared {
            block_on(child.map_shared(m.addr, m.len, &*m.pages, m.offset, m.writable)).unwrap();
        }
        child
    }

    fn range(&self, addr: u32, len: usize) -> Result<*mut u8, Fault> {
        match (addr as usize).checked_add(len) {
            Some(end) if end <= self.size.get() => {
                // SAFETY: in bounds, checked above.
                Ok(unsafe { self.base.add(addr as usize) })
            }
            _ => Err(Fault),
        }
    }

    /// The host address of `len` bytes at `addr`, which may be written: outside every read-only
    /// view.
    fn writable_range(&self, addr: u32, len: usize) -> Result<*mut u8, Fault> {
        let end = addr as usize + len;
        let read_only = self.read_only.borrow();
        let hit = |r: &Range<u32>| (r.start as usize) < end && addr < r.end;
        match len > 0 && read_only.iter().any(hit) {
            true => Err(Fault),
            false => self.range(addr, len),
        }
    }

    /// Records `[addr, addr + len)` as a read-only view or not.
    fn set_read_only(&self, addr: u32, len: u64, read_only: bool) {
        let end = addr + len as u32;
        let mut ranges = self.read_only.borrow_mut();
        let old = std::mem::take(&mut *ranges);
        for r in old {
            let parts = [r.start..r.end.min(addr), r.start.max(end)..r.end];
            ranges.extend(parts.into_iter().filter(|part| !part.is_empty()));
        }
        if read_only {
            ranges.push(addr..end);
        }
    }

    pub(super) fn get(&self, addr: u32, len: usize) -> Vec<u8> {
        let mut buf = vec![0; len];
        block_on(self.read(addr, &mut buf)).unwrap();
        buf
    }

    pub(super) fn put(&self, addr: u32, data: &[u8]) {
        block_on(self.write(addr, data)).unwrap();
    }
}

impl Drop for MappedMemory {
    fn drop(&mut self) {
        // SAFETY: the reservation this value owns, with whatever was mapped inside it.
        unsafe { libc::munmap(self.base.cast(), RESERVED * PAGE as usize) };
    }
}

impl UserMemory for MappedMemory {
    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let result = self.range(addr, buf.len()).map(|src| {
            // SAFETY: in bounds; nothing else holds a reference into the memory.
            unsafe { std::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), buf.len()) }
        });
        Box::pin(async move { result })
    }

    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        let result = self.writable_range(addr, data.len()).map(|dst| {
            // SAFETY: in bounds; nothing else holds a reference into the memory.
            unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len()) }
        });
        Box::pin(async move { result })
    }

    fn size(&self) -> LocalFuture<'_, u64> {
        let size = self.size.get() as u64;
        Box::pin(async move { size })
    }

    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>> {
        let previous = self.size.get();
        let grown = previous + pages as usize * PAGE as usize;
        let fits = grown <= RESERVED * PAGE as usize;
        if fits {
            self.size.set(grown);
        }
        Box::pin(async move { fits.then_some(previous as u64) })
    }

    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        let forkable = self.forkable;
        Box::pin(async move { forkable })
    }

    fn compare_exchange_u32(
        &self,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> LocalFuture<'_, Result<u32, Fault>> {
        let result = self.writable_range(addr, 4).map(|ptr| {
            // SAFETY: in bounds and aligned (futex words are); other memories reach the word
            // only through atomics or copies.
            let word = unsafe { AtomicU32::from_ptr(ptr.cast()) };
            let current = word.compare_exchange(expected, new, Ordering::SeqCst, Ordering::SeqCst);
            current.unwrap_or_else(|actual| actual)
        });
        Box::pin(async move { result })
    }

    fn map_shared<'a>(
        &'a self,
        addr: u32,
        len: u64,
        pages: &'a dyn SharedPages,
        offset: u64,
        writable: bool,
    ) -> LocalFuture<'a, Result<(), Errno>> {
        let result = self.range(addr, len as usize).map_err(|_| Errno::EINVAL);
        // SAFETY: inside the reservation this memory owns; nothing writes a read-only view.
        let result = result.and_then(|ptr| {
            unsafe { map_at(ptr, len as usize, pages, offset, writable) }.map_err(|_| Errno::EINVAL)
        });
        if result.is_ok() {
            self.set_read_only(addr, len, !writable);
        }
        Box::pin(async move { result })
    }

    fn unmap_shared(&self, addr: u32, len: u64) -> LocalFuture<'_, Result<(), Errno>> {
        let result = self.range(addr, len as usize).map_err(|_| Errno::EINVAL);
        // SAFETY: inside the reservation this memory owns.
        let result = result.and_then(|ptr| {
            unsafe { map_private_at(ptr, len as usize) }.map_err(|_| Errno::EINVAL)
        });
        if result.is_ok() {
            self.set_read_only(addr, len, false);
        }
        Box::pin(async move { result })
    }
}

/// [`TestHost`] with shared memory objects, as a host on Linux or macOS has.
pub(super) struct SharedHost;

impl Host for SharedHost {
    fn realtime(&self) -> Duration {
        TestHost.realtime()
    }

    fn monotonic(&self) -> Duration {
        TestHost.monotonic()
    }

    fn sleep(&self, duration: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async move { TestHost.sleep(duration).await })
    }

    fn sleep_until(&self, deadline: Duration) -> LocalFuture<'_, ()> {
        Box::pin(async move { TestHost.sleep_until(deadline).await })
    }

    fn random(&self, buf: &mut [u8]) {
        TestHost.random(buf)
    }

    fn executor(&self) -> &dyn Executor {
        &TestHost
    }

    fn shared_pages(&self, size: u64) -> io::Result<Arc<dyn SharedPages>> {
        Ok(Arc::new(Memfd::new(size)?))
    }
}

/// Keeps the files of a memory filesystem in memfds, as `/dev/shm` and the memfd filesystem
/// do on such a host.
impl Provider for SharedHost {
    fn shared_pages(&self, size: u64) -> io::Result<Arc<dyn SharedPages>> {
        Host::shared_pages(self, size)
    }
}

/// A kernel on [`SharedHost`] with one task, whose memfds live in host shared memory.
pub(super) fn shared_machine() -> (Kernel, TaskId) {
    let host: Arc<dyn Host> = Arc::new(SharedHost);
    let clock = Arc::new(HostClock(host.clone()));
    let root = MemFs::new(clock.clone(), 0o755, 1, 1);
    let shm = MemFs::new(clock, 0o1777, 0, 0).with_shared_pages(Arc::new(SharedHost));
    let kernel = Kernel::new(host, Arc::new(root)).with_shm(Arc::new(shm));
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, task, _) = spawn_on(kernel, stdin, &[], &[]);
    (kernel, task)
}

fn mmap_shared(kernel: &Kernel, caller: &Caller<'_>, len: u32) -> u32 {
    let flags = MapFlags::MAP_SHARED | MapFlags::MAP_ANONYMOUS;
    block_on(kernel.mmap(caller, 0, len, RW, flags, Fd(-1), 0)).unwrap()
}

/// A second mapping of the shared mapping at `addr` (`mremap` with an old length of 0).
fn duplicate(kernel: &Kernel, caller: &Caller<'_>, addr: u32, len: u32) -> SysResult<i32> {
    let flags = MremapFlags::MREMAP_MAYMOVE;
    block_on(kernel.mremap(caller, addr, 0, len, flags, 0))
}

#[test]
fn two_mappings_of_shared_anonymous_memory_see_each_others_stores_at_once() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, 2 * PAGE);
    let b = duplicate(&kernel, &caller, a, 2 * PAGE).unwrap() as u32;
    assert_ne!(a, b);

    // No syscall in between: the two ranges are the same pages.
    memory.put(a + PAGE + 4, b"seen");
    assert_eq!(memory.get(b + PAGE + 4, 4), b"seen");
    memory.put(b, b"back");
    assert_eq!(memory.get(a, 4), b"back");
}

#[test]
fn munmap_leaves_zeroed_private_pages() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, 2 * PAGE);
    let b = duplicate(&kernel, &caller, a, 2 * PAGE).unwrap() as u32;
    memory.put(a + PAGE, b"x");

    assert_eq!(block_on(kernel.munmap(&caller, a + PAGE, PAGE)), Ok(0));
    // The unmapped page is the memory's own again, and zero; the object kept the byte.
    assert_eq!(memory.get(a + PAGE, 1), [0]);
    memory.put(a + PAGE, b"y");
    assert_eq!(memory.get(b + PAGE, 1), b"x");
    // The page left mapped still shows the object.
    memory.put(a, b"z");
    assert_eq!(memory.get(b, 1), b"z");
}

#[test]
fn mremap_moves_the_objects_pages_along() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, PAGE);
    let b = duplicate(&kernel, &caller, a, PAGE).unwrap() as u32;
    memory.put(a, b"old");
    // Growing past `b` cannot happen in place: the mapping moves.
    let flags = MremapFlags::MREMAP_MAYMOVE;
    let moved = block_on(kernel.mremap(&caller, a, PAGE, 2 * PAGE, flags, 0)).unwrap() as u32;
    assert_ne!(moved, a);

    assert_eq!(memory.get(moved, 3), b"old");
    memory.put(moved, b"new");
    assert_eq!(memory.get(b, 3), b"new");
    // The old range is the memory's own again.
    assert_eq!(memory.get(a, 3), [0; 3]);
    // The object has one page: the page past it is anonymous memory.
    memory.put(moved + PAGE, b"tail");
    assert_eq!(block_on(kernel.munmap(&caller, b, PAGE)), Ok(0));
    assert_eq!(memory.get(moved, 3), b"new");
}

#[test]
fn madvise_dontneed_keeps_a_mapped_objects_contents() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, PAGE);
    memory.put(a, b"kept");
    let advice = MadviseAdvice::MADV_DONTNEED;
    assert_eq!(block_on(kernel.madvise(&caller, a, PAGE, advice)), Ok(0));
    assert_eq!(memory.get(a, 4), b"kept");
}

#[test]
fn madvise_does_not_know_memory_failure_advice() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, PAGE);
    for advice in [
        MadviseAdvice::MADV_HWPOISON,
        MadviseAdvice::MADV_SOFT_OFFLINE,
    ] {
        assert_eq!(
            block_on(kernel.madvise(&caller, a, PAGE, advice)),
            Err(Errno::EINVAL.into()),
            "{advice:?}"
        );
    }
}

#[test]
fn memory_that_cannot_show_pages_keeps_shared_anonymous_memory_private() {
    // The host has objects, but the memory cannot place them: the mapping is ordinary memory,
    // which cannot be duplicated.
    let (kernel, task) = shared_machine();
    let memory = FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize]));
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, PAGE);
    assert_eq!(errno(duplicate(&kernel, &caller, a, PAGE)), Errno::EINVAL);

    // A host without objects: the same, over memory that could show them.
    let (kernel, task, _) = setup(b"");
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, PAGE);
    assert_eq!(errno(duplicate(&kernel, &caller, a, PAGE)), Errno::EINVAL);
}

fn fork(kernel: &Kernel, caller: &Caller<'_>) -> TaskId {
    let flags = wasixv2_abi::Signal::SIGCHLD.0 as u32;
    let none = wasixv2_abi::S32Ptr::new(0);
    let child = block_on(kernel.clone(caller, flags, 0, none, 0, none));
    while kernel.take_pending(caller.task()).is_some() {}
    TaskId(child.expect("fork") as u32)
}

#[test]
fn shared_anonymous_memory_stays_shared_across_fork() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let private = block_on(kernel.mmap(
        &caller,
        0,
        PAGE,
        RW,
        MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS,
        Fd(-1),
        0,
    ))
    .unwrap();
    let shared = mmap_shared(&kernel, &caller, 2 * PAGE);
    memory.put(shared, b"before");
    memory.put(private, b"before");

    let child = fork(&kernel, &caller);
    let inherited = kernel.shared_mappings(child);
    assert_eq!(inherited.len(), 1);
    assert_eq!(
        (inherited[0].addr, inherited[0].len),
        (shared, 2 * u64::from(PAGE))
    );
    let child_memory = memory.fork(&inherited);
    assert_eq!(child_memory.get(shared, 6), b"before");

    // Shared memory: each sees the other's stores at once, with no syscall in between.
    child_memory.put(shared + PAGE, b"from the child");
    assert_eq!(memory.get(shared + PAGE, 14), b"from the child");
    memory.put(shared, b"parent");
    assert_eq!(child_memory.get(shared, 6), b"parent");
    // Private memory: each has its own copy.
    child_memory.put(private, b"child!");
    assert_eq!(memory.get(private, 6), b"before");

    // Unmapping in the child leaves the parent's mapping alone.
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(
        block_on(kernel.munmap(&child_caller, shared, 2 * PAGE)),
        Ok(0)
    );
    assert!(kernel.shared_mappings(child).is_empty());
    memory.put(shared, b"still");
    assert_eq!(child_memory.get(shared, 5), [0; 5]);
    assert_eq!(kernel.shared_mappings(parent).len(), 1);
}

/// `futex` with no timeout and no second address, to poll.
fn futex<'a>(
    kernel: &'a Kernel,
    caller: &'a Caller<'a>,
    addr: u32,
    op: FutexOp,
    val: u32,
) -> LocalFuture<'a, SysResult<i32>> {
    let (uaddr, null) = (GuestPtrMut::new(addr), GuestPtrMut::NULL);
    Box::pin(kernel.futex_time64(caller, uaddr, op, val, 0, null, 0))
}

#[test]
fn futex_wake_crosses_processes_on_a_shared_mapping() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let word = mmap_shared(&kernel, &caller, PAGE) + 64;
    let child = fork(&kernel, &caller);
    let child_memory = memory.fork(&kernel.shared_mappings(child));
    let child_caller = Caller::new(child, &child_memory);

    let mut wait = futex(&kernel, &child_caller, word, FutexOp::FUTEX_WAIT, 0);
    assert_eq!(wait.as_mut().now_or_never(), None);
    // A private wake looks in the parent's own address space only.
    let private = futex(&kernel, &caller, word, FutexOp::FUTEX_WAKE_PRIVATE, 1);
    assert_eq!(private.now_or_never(), Some(Ok(0)));
    assert_eq!(
        futex(&kernel, &caller, word, FutexOp::FUTEX_WAKE, 1).now_or_never(),
        Some(Ok(1))
    );
    assert_eq!(wait.now_or_never(), Some(Ok(0)));

    // The instructions meet the syscall there too.
    let mut wait = AtomicWait::atomic_wait(&kernel, &caller, word, AtomicWord::U32(0), None);
    assert_eq!(wait.as_mut().now_or_never(), None);
    assert_eq!(AtomicWait::atomic_notify(&kernel, child, word, 1), 1);
    assert_eq!(wait.now_or_never(), Some(Ok(0)));
}

#[test]
fn a_private_futex_on_a_shared_mapping_stays_in_its_address_space() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let word = mmap_shared(&kernel, &caller, PAGE);
    let child = fork(&kernel, &caller);
    let child_memory = memory.fork(&kernel.shared_mappings(child));
    let child_caller = Caller::new(child, &child_memory);

    let mut wait = futex(&kernel, &child_caller, word, FutexOp::FUTEX_WAIT_PRIVATE, 0);
    assert_eq!(wait.as_mut().now_or_never(), None);
    for op in [FutexOp::FUTEX_WAKE, FutexOp::FUTEX_WAKE_PRIVATE] {
        assert_eq!(
            futex(&kernel, &caller, word, op, 1).now_or_never(),
            Some(Ok(0))
        );
    }
    let wake = FutexOp::FUTEX_WAKE_PRIVATE;
    let woken = futex(&kernel, &child_caller, word, wake, 1).now_or_never();
    assert_eq!(woken, Some(Ok(1)));
    assert_eq!(wait.now_or_never(), Some(Ok(0)));
}

#[test]
fn one_object_mapped_at_two_addresses_is_one_futex() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let a = mmap_shared(&kernel, &caller, PAGE);
    let b = duplicate(&kernel, &caller, a, PAGE).unwrap() as u32;

    let mut wait = futex(&kernel, &caller, a + 8, FutexOp::FUTEX_WAIT, 0);
    assert_eq!(wait.as_mut().now_or_never(), None);
    let woken = futex(&kernel, &caller, b + 8, FutexOp::FUTEX_WAKE, 1).now_or_never();
    assert_eq!(woken, Some(Ok(1)));
    assert_eq!(wait.now_or_never(), Some(Ok(0)));
}

#[test]
fn a_pi_futex_is_handed_over_across_processes() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let lock = mmap_shared(&kernel, &caller, PAGE);
    let child = fork(&kernel, &caller);
    let child_memory = memory.fork(&kernel.shared_mappings(child));
    let child_caller = Caller::new(child, &child_memory);

    let locked = futex(&kernel, &caller, lock, FutexOp::FUTEX_LOCK_PI, 0).now_or_never();
    assert_eq!(locked, Some(Ok(0)));
    assert_eq!(child_memory.get(lock, 4), parent.0.to_le_bytes());
    let mut wait = futex(&kernel, &child_caller, lock, FutexOp::FUTEX_LOCK_PI, 0);
    assert_eq!(wait.as_mut().now_or_never(), None);
    let waiters = 0x8000_0000 | parent.0;
    assert_eq!(memory.get(lock, 4), waiters.to_le_bytes());

    let unlocked = futex(&kernel, &caller, lock, FutexOp::FUTEX_UNLOCK_PI, 0).now_or_never();
    assert_eq!(unlocked, Some(Ok(0)));
    assert_eq!(wait.now_or_never(), Some(Ok(0)));
    let owner = 0x8000_0000 | child.0;
    assert_eq!(memory.get(lock, 4), owner.to_le_bytes());
}

// ---- SysV segments ----

const CREATE: i32 = wasixv2_abi::IpcFlags::IPC_CREAT.0 as i32 | 0o600;

fn nattch(kernel: &Kernel, caller: &Caller<'_>, id: i32) -> u32 {
    let buf = GuestPtrMut::new(0x700);
    block_on(kernel.shmctl(caller, id, IpcCmd::IPC_STAT.0, buf)).unwrap();
    let ds: Shmid64Ds = block_on(read_value(caller.memory(), GuestPtr::new(0x700))).unwrap();
    ds.shm_nattch
}

#[test]
fn two_processes_see_each_others_segment_stores_without_a_syscall() {
    let (kernel, a) = shared_machine();
    let b = super::mapping::another_process(&kernel);
    let (memory_a, memory_b) = (MappedMemory::new(2, false), MappedMemory::new(2, false));
    let (caller_a, caller_b) = (Caller::new(a, &memory_a), Caller::new(b, &memory_b));
    let id = block_on(kernel.shmget(&caller_a, 5, 2 * PAGE, CREATE)).unwrap();
    let at_a = block_on(kernel.shmat(&caller_a, id, 0, 0)).unwrap() as u32;
    let at_b = block_on(kernel.shmat(&caller_b, id, 0, 0)).unwrap() as u32;
    assert_eq!(nattch(&kernel, &caller_a, id), 2);

    memory_a.put(at_a + PAGE - 2, b"from a");
    assert_eq!(memory_b.get(at_b + PAGE - 2, 6), b"from a");
    // A futex in the segment works between the processes.
    let mut wait = futex(&kernel, &caller_b, at_b + 16, FutexOp::FUTEX_WAIT, 0);
    assert_eq!(wait.as_mut().now_or_never(), None);
    let woken = futex(&kernel, &caller_a, at_a + 16, FutexOp::FUTEX_WAKE, 1).now_or_never();
    assert_eq!(woken, Some(Ok(1)));
    assert_eq!(wait.now_or_never(), Some(Ok(0)));

    assert_eq!(block_on(kernel.shmdt(&caller_b, at_b)), Ok(0));
    assert_eq!(memory_b.get(at_b + PAGE - 2, 6), [0; 6]);
    assert_eq!(nattch(&kernel, &caller_a, id), 1);
    assert!(kernel.shared_mappings(b).is_empty());
    assert_eq!(memory_a.get(at_a + PAGE - 2, 6), b"from a");
}

#[test]
fn a_fork_child_inherits_a_mapped_attachment() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let id = block_on(kernel.shmget(&caller, 6, PAGE, CREATE)).unwrap();
    let at = block_on(kernel.shmat(&caller, id, 0, 0)).unwrap() as u32;
    let child = fork(&kernel, &caller);
    let child_memory = memory.fork(&kernel.shared_mappings(child));
    let child_caller = Caller::new(child, &child_memory);
    assert_eq!(nattch(&kernel, &caller, id), 2);

    child_memory.put(at, b"child");
    assert_eq!(memory.get(at, 5), b"child");
    assert_eq!(block_on(kernel.shmdt(&child_caller, at)), Ok(0));
    assert_eq!(nattch(&kernel, &caller, id), 1);
    memory.put(at, b"after");
    assert_eq!(child_memory.get(at, 5), [0; 5]);
}

#[test]
fn a_copied_attachment_meets_a_mapped_one_at_its_sync_points() {
    // A's memory cannot show the segment's pages, so its attachment is a copy of them.
    let (kernel, a) = shared_machine();
    let b = super::mapping::another_process(&kernel);
    let memory_a = FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize]));
    let memory_b = MappedMemory::new(2, false);
    let (caller_a, caller_b) = (Caller::new(a, &memory_a), Caller::new(b, &memory_b));
    let id = block_on(kernel.shmget(&caller_a, 7, PAGE, CREATE)).unwrap();
    let at_a = block_on(kernel.shmat(&caller_a, id, 0, 0)).unwrap() as u32;
    let at_b = block_on(kernel.shmat(&caller_b, id, 0, 0)).unwrap() as u32;
    assert!(kernel.shared_mappings(a).is_empty());

    super::ipc::put(&memory_a, at_a, b"copied");
    assert_eq!(memory_b.get(at_b, 6), [0; 6]);
    assert_eq!(block_on(kernel.shmdt(&caller_a, at_a)), Ok(0));
    assert_eq!(memory_b.get(at_b, 6), b"copied");
    memory_b.put(at_b, b"mapped");
    let at_a = block_on(kernel.shmat(&caller_a, id, 0, 0)).unwrap() as u32;
    assert_eq!(super::ipc::get(&memory_a, at_a, 6), b"mapped");
}

#[test]
fn a_read_only_mapped_attachment_never_becomes_writable() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let id = block_on(kernel.shmget(&caller, 8, PAGE, CREATE)).unwrap();
    let rdonly = wasixv2_abi::ShmFlags::SHM_RDONLY.0 as i32;
    let at = block_on(kernel.shmat(&caller, id, 0, rdonly)).unwrap() as u32;
    assert_eq!(kernel.shared_mappings(task).len(), 1);
    let write = ProtFlags::PROT_READ | ProtFlags::PROT_WRITE;
    assert_eq!(
        errno(block_on(kernel.mprotect(&caller, at, PAGE, write))),
        Errno::EACCES
    );
}

const NAME: u32 = 0x100;
const DATA: u32 = 0x1000;

fn memfd(kernel: &Kernel, caller: &Caller<'_>, flags: MemfdFlags) -> Fd {
    block_on(caller.memory().write(NAME, b"shm\0")).unwrap();
    block_on(kernel.memfd_create(caller, Cstr::new(NAME), flags)).unwrap()
}

fn map_file(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, len: u32, prot: ProtFlags) -> u32 {
    let mapped = kernel.mmap(caller, 0, len, prot, MapFlags::MAP_SHARED, fd, 0);
    block_on(mapped).unwrap()
}

fn pwrite(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, data: &[u8], pos: i64) {
    block_on(caller.memory().write(DATA, data)).unwrap();
    let slice = UserSlice::new(DATA, data.len() as u32);
    let written = block_on(kernel.pwrite64(caller, fd, slice, pos));
    assert_eq!(written, Ok(data.len() as u32));
}

fn pread(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, len: u32, pos: i64) -> Vec<u8> {
    let read = block_on(kernel.pread64(caller, fd, UserSliceMut::new(DATA, len), pos));
    let mut buf = vec![0; read.unwrap() as usize];
    block_on(caller.memory().read(DATA, &mut buf)).unwrap();
    buf
}

fn ftruncate(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, len: i64) {
    assert_eq!(block_on(kernel.ftruncate64(caller, fd, len)), Ok(0));
}

fn add_seals(kernel: &Kernel, caller: &Caller<'_>, fd: Fd, seals: SealFlags) -> SysResult<i32> {
    block_on(kernel.fcntl64(caller, fd, FcntlCmd::F_ADD_SEALS, seals.0))
}

#[test]
fn a_memfd_mapping_is_the_files_pages_in_every_process() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let fd = memfd(&kernel, &caller, MemfdFlags::empty());
    ftruncate(&kernel, &caller, fd, 2 * i64::from(PAGE));
    let at = map_file(&kernel, &caller, fd, 2 * PAGE, RW);
    assert_eq!(kernel.shared_mappings(parent).len(), 1);

    // write() and the mapping are the same bytes, with no synchronisation point in between.
    pwrite(&kernel, &caller, fd, b"written", 8);
    assert_eq!(memory.get(at + 8, 7), b"written");
    memory.put(at + PAGE, b"stored");
    assert_eq!(pread(&kernel, &caller, fd, 6, PAGE.into()), b"stored");

    let child = fork(&kernel, &caller);
    let child_memory = memory.fork(&kernel.shared_mappings(child));
    child_memory.put(at, b"child");
    assert_eq!(memory.get(at, 5), b"child");
    memory.put(at + 2 * PAGE - 6, b"parent");
    assert_eq!(child_memory.get(at + 2 * PAGE - 6, 6), b"parent");
}

#[test]
fn ftruncate_under_a_mapping_zeroes_the_tail_and_keeps_it_mapped() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let fd = memfd(&kernel, &caller, MemfdFlags::empty());
    // Mapped past the end of an empty file: Linux would raise SIGBUS there; here the pages
    // are backed and read zero.
    let at = map_file(&kernel, &caller, fd, 2 * PAGE, RW);
    assert_eq!(memory.get(at + PAGE, 4), [0; 4]);

    ftruncate(&kernel, &caller, fd, 2 * i64::from(PAGE));
    memory.put(at, &[7; 16]);
    memory.put(at + PAGE, b"tail");
    ftruncate(&kernel, &caller, fd, 4);
    assert_eq!(memory.get(at, 6), [7, 7, 7, 7, 0, 0]);
    assert_eq!(memory.get(at + PAGE, 4), [0; 4]);

    // Growing again shows zeros, and the mapping still is the file.
    ftruncate(&kernel, &caller, fd, 2 * i64::from(PAGE));
    assert_eq!(memory.get(at + PAGE, 4), [0; 4]);
    memory.put(at + PAGE, b"again");
    assert_eq!(pread(&kernel, &caller, fd, 5, PAGE.into()), b"again");
}

#[test]
fn a_write_seal_waits_for_every_writable_mapping_in_every_process() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let fd = memfd(&kernel, &caller, MemfdFlags::MFD_ALLOW_SEALING);
    ftruncate(&kernel, &caller, fd, PAGE.into());
    let at = map_file(&kernel, &caller, fd, PAGE, RW);
    let child = fork(&kernel, &caller);
    let child_memory = memory.fork(&kernel.shared_mappings(child));
    let child_caller = Caller::new(child, &child_memory);

    let write = SealFlags::F_SEAL_WRITE;
    assert_eq!(errno(add_seals(&kernel, &caller, fd, write)), Errno::EBUSY);
    assert_eq!(block_on(kernel.munmap(&caller, at, PAGE)), Ok(0));
    // The child's copy of the mapping still counts.
    assert_eq!(errno(add_seals(&kernel, &caller, fd, write)), Errno::EBUSY);
    assert_eq!(block_on(kernel.munmap(&child_caller, at, PAGE)), Ok(0));
    assert_eq!(add_seals(&kernel, &caller, fd, write), Ok(0));

    let mapped = kernel.mmap(&caller, 0, PAGE, RW, MapFlags::MAP_SHARED, fd, 0);
    assert_eq!(errno(block_on(mapped)), Errno::EPERM);
    // A read-only mapping of the sealed file shows its pages read-only: stores to it fault.
    let read_only = map_file(&kernel, &caller, fd, PAGE, ProtFlags::PROT_READ);
    let shared = kernel.shared_mappings(parent);
    assert!(shared.len() == 1 && !shared[0].writable);
    assert_eq!(block_on(memory.write(read_only, b"sealed?")), Err(Fault));
    let synced = kernel.msync(&caller, read_only, PAGE, MsyncFlags::MS_SYNC);
    assert_eq!(block_on(synced), Ok(0));
    assert_eq!(pread(&kernel, &caller, fd, 7, 0), [0; 7]);
}

#[test]
fn a_read_only_attachment_shows_the_segment_and_takes_no_store() {
    let (kernel, parent) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(parent, &memory);
    let id = block_on(kernel.shmget(&caller, 8, PAGE, CREATE)).unwrap();
    let rdonly = wasixv2_abi::ShmFlags::SHM_RDONLY.0 as i32;
    let rw = block_on(kernel.shmat(&caller, id, 0, 0)).unwrap() as u32;
    let ro = block_on(kernel.shmat(&caller, id, 0, rdonly)).unwrap() as u32;

    // The view shows every store, with no synchronisation point in between.
    memory.put(rw + 8, b"stored");
    assert_eq!(memory.get(ro + 8, 6), b"stored");
    assert_eq!(block_on(memory.write(ro + 8, b"x")), Err(Fault));
    // A syscall that would store there fails as copy_to_user does.
    let fd = memfd(&kernel, &caller, MemfdFlags::empty());
    pwrite(&kernel, &caller, fd, b"file", 0);
    let read = block_on(kernel.pread64(&caller, fd, UserSliceMut::new(ro + 8, 4), 0));
    assert_eq!(errno(read), Errno::EFAULT);
    assert_eq!(memory.get(ro + 8, 6), b"stored");

    // A fork child's copy of the attachment is read-only as well.
    let child = fork(&kernel, &caller);
    let child_memory = memory.fork(&kernel.shared_mappings(child));
    assert_eq!(block_on(child_memory.write(ro, b"x")), Err(Fault));
    child_memory.put(rw, b"child");
    assert_eq!(memory.get(ro, 5), b"child");

    // Detached, the range is writable memory again.
    assert_eq!(block_on(kernel.shmdt(&caller, ro)), Ok(0));
    memory.put(ro, b"private");
    assert_eq!(memory.get(rw, 5), b"child");
}

#[test]
fn a_shared_mapping_of_a_file_open_for_reading_shows_its_pages_read_only() {
    let (kernel, task) = shared_machine();
    let clock = Arc::new(HostClock(kernel.host.clone()));
    let shm = MemFs::new(clock, 0o1777, 0, 0).with_shared_pages(Arc::new(SharedHost));
    let vfs = kernel.vfs().clone();
    block_on(vfs.mount(b"/shm", Arc::new(shm), MountFlags::default(), true)).unwrap();
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let open = |flags: OpenFlags| {
        block_on(caller.memory().write(NAME, b"/shm/f\0")).unwrap();
        block_on(kernel.openat(&caller, Fd(-100), Cstr::new(NAME), flags, 0o600)).unwrap()
    };
    let writer = open(OpenFlags::O_RDWR | OpenFlags::O_CREAT);
    ftruncate(&kernel, &caller, writer, PAGE.into());
    let reader = open(OpenFlags::O_RDONLY);

    let at = map_file(&kernel, &caller, reader, PAGE, ProtFlags::PROT_READ);
    let shared = kernel.shared_mappings(task);
    assert!(shared.len() == 1 && !shared[0].writable);
    let rw_at = map_file(&kernel, &caller, writer, PAGE, RW);
    memory.put(rw_at + 16, b"stored");
    assert_eq!(memory.get(at + 16, 6), b"stored");
    pwrite(&kernel, &caller, writer, b"written", 0);
    assert_eq!(memory.get(at, 7), b"written");

    assert_eq!(block_on(memory.write(at, b"x")), Err(Fault));
    let read = block_on(kernel.pread64(&caller, writer, UserSliceMut::new(at, 4), 0));
    assert_eq!(errno(read), Errno::EFAULT);
    let mprotect = kernel.mprotect(&caller, at, PAGE, RW);
    assert_eq!(errno(block_on(mprotect)), Errno::EACCES);

    // A mapping placed over it takes its write protection away, as munmap does.
    let fixed = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS | MapFlags::MAP_FIXED;
    let anon = block_on(kernel.mmap(&caller, at, PAGE, RW, fixed, Fd(-1), 0));
    assert_eq!(anon, Ok(at));
    assert!(kernel.shared_mappings(task).len() == 1);
    memory.put(at, b"private");
    assert_eq!(memory.get(rw_at, 7), b"written");
}

#[test]
fn memory_that_cannot_show_pages_copies_a_memfd_mapping() {
    let (kernel, task) = shared_machine();
    let memory = FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize]));
    let caller = Caller::new(task, &memory);
    let fd = memfd(&kernel, &caller, MemfdFlags::empty());
    ftruncate(&kernel, &caller, fd, PAGE.into());
    let at = map_file(&kernel, &caller, fd, PAGE, RW);
    assert!(kernel.shared_mappings(task).is_empty());

    // The copy meets the file at its synchronisation points, as any file mapping.
    block_on(memory.write(at + 4, b"copied")).unwrap();
    assert_eq!(pread(&kernel, &caller, fd, 6, 4), b"copied");
    pwrite(&kernel, &caller, fd, b"back", 0);
    let mut buf = [0; 4];
    block_on(memory.read(at, &mut buf)).unwrap();
    assert_eq!(&buf, b"back");
}

#[test]
fn host_mapped_files_retain_core_note_provenance_after_splitting() {
    let (kernel, task) = shared_machine();
    let memory = MappedMemory::new(2, true);
    let caller = Caller::new(task, &memory);
    memory.put(0x100, b"core-file\0");
    let fd = block_on(kernel.memfd_create(&caller, Cstr::new(0x100), MemfdFlags(0))).unwrap();
    block_on(kernel.ftruncate64(&caller, fd, i64::from(3 * PAGE))).unwrap();
    let addr = block_on(kernel.mmap(
        &caller,
        0,
        2 * PAGE,
        RW,
        MapFlags::MAP_SHARED,
        fd,
        i64::from(PAGE),
    ))
    .unwrap();
    block_on(kernel.munmap(&caller, addr, PAGE)).unwrap();
    let files = kernel.mm(task).file_regions();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].range,
        u64::from(addr + PAGE)..u64::from(addr + 2 * PAGE)
    );
    assert_eq!(files[0].offset, u64::from(2 * PAGE));
    assert!(Arc::ptr_eq(&files[0].file, &kernel.file(task, fd).unwrap()));
}

#[test]
fn tmpfs_mapping_quota_covers_mremap_growth_and_duplicates_before_changing_memory() {
    use wasixv2_abi::{IfDqblk, QuotaMisc};
    use wasixv2_vfs::QuotaOps;
    use wasixv2_vfs_mem::{Limits, QuotaLimits};

    let host: Arc<dyn Host> = Arc::new(SharedHost);
    let clock = Arc::new(HostClock(host.clone()));
    let root = MemFs::new(clock.clone(), 0o755, 1, 1);
    let shm = Arc::new(
        MemFs::with_limits_and_quotas(
            clock,
            0o1777,
            0,
            0,
            Limits::UNLIMITED,
            QuotaLimits {
                types: 1,
                block_hard: [u64::from(PAGE), 0],
                ..QuotaLimits::default()
            },
        )
        .unwrap()
        .with_shared_pages(Arc::new(SharedHost)),
    );
    let kernel = Kernel::new(host, Arc::new(root)).with_shm(shm.clone());
    let (kernel, task, _) = spawn_on(kernel, Arc::new(Input(Mutex::new(Vec::new()))), &[], &[]);
    let memory = MappedMemory::new(2, false);
    let caller = Caller::new(task, &memory);
    let fd = memfd(&kernel, &caller, MemfdFlags::empty());
    ftruncate(&kernel, &caller, fd, i64::from(2 * PAGE));
    let a = map_file(&kernel, &caller, fd, PAGE, RW);
    memory.put(a, b"kept");
    assert_eq!(shm.get(0, 1).unwrap().dqb_curspace, u64::from(PAGE));
    assert_eq!(
        duplicate(&kernel, &caller, a, 2 * PAGE),
        Err(SysErr::Errno(Errno::EDQUOT))
    );
    let remap =
        || block_on(kernel.mremap(&caller, a, PAGE, 2 * PAGE, MremapFlags::MREMAP_MAYMOVE, 0));
    assert_eq!(remap(), Err(SysErr::Errno(Errno::EDQUOT)));
    assert_eq!(memory.get(a, 4), b"kept");
    let mappings = kernel.shared_mappings(task);
    assert_eq!(mappings.len(), 1);
    assert_eq!((mappings[0].addr, mappings[0].len), (a, u64::from(PAGE)));
    drop(mappings);
    shm.set(
        0,
        1,
        &IfDqblk {
            dqb_bhardlimit: u64::from(2 * PAGE / 1024),
            dqb_valid: QuotaMisc::QIF_BLIMITS.0,
            ..IfDqblk::default()
        },
    )
    .unwrap();
    let grown = remap().unwrap() as u32;
    assert_eq!(shm.get(0, 1).unwrap().dqb_curspace, u64::from(2 * PAGE));
    memory.put(grown + PAGE, b"grown");
    assert_eq!(pread(&kernel, &caller, fd, 5, i64::from(PAGE)), b"grown");
    let alias = duplicate(&kernel, &caller, grown, 2 * PAGE).unwrap() as u32;
    assert_eq!(memory.get(alias + PAGE, 5), b"grown");
    assert_eq!(shm.get(0, 1).unwrap().dqb_curspace, u64::from(2 * PAGE));
}

#[test]
fn shmdt_detaches_retained_pages_before_address_reuse() {
    for mapped in [false, true] {
        let (kernel, task) = shared_machine();
        let memory: Box<dyn UserMemory> = if mapped {
            Box::new(MappedMemory::new(2, false))
        } else {
            Box::new(FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize])))
        };
        let caller = Caller::new(task, &*memory);
        let id = block_on(kernel.shmget(&caller, 0, PAGE, CREATE)).unwrap();
        let at = block_on(kernel.shmat(&caller, id, 0, 0)).unwrap() as u32;
        block_on(memory.write(at, b"old!")).unwrap();
        let retained = block_on(kernel.retain_pages(&caller, at, 4)).unwrap();
        assert_eq!(block_on(kernel.shmdt(&caller, at)), Ok(0));
        let flags = MapFlags::MAP_ANONYMOUS | MapFlags::MAP_PRIVATE | MapFlags::MAP_FIXED;
        assert_eq!(
            block_on(kernel.mmap(&caller, at, PAGE, RW, flags, Fd(-1), 0)),
            Ok(at)
        );
        block_on(memory.write(at, b"new!")).unwrap();
        let replacement = block_on(kernel.retain_pages(&caller, at, 4)).unwrap();
        let mut bytes = [0; 4];
        block_on(retained.read(&*memory, 0, &mut bytes)).unwrap();
        assert_eq!(&bytes, b"old!", "detached mapped={mapped}");
        block_on(replacement.read(&*memory, 0, &mut bytes)).unwrap();
        assert_eq!(&bytes, b"new!", "replacement mapped={mapped}");
    }
}

/// mm/shmem.c:shmem_mmap_prepare calls file_accessed even when mmap installs
/// shared pages without reading the file. Failed validation must not access it.
#[test]
fn file_mmap_accesses_tmpfs_after_validation_and_honors_noatime() {
    use wasixv2_vfs::{SetAttr, TimeSet, Timestamp};
    for flags in [MapFlags::MAP_SHARED, MapFlags::MAP_PRIVATE] {
        for noatime in [false, true] {
            let host: Arc<dyn Host> = Arc::new(SharedHost);
            let clock = Arc::new(HostClock(host.clone()));
            let root = MemFs::new(clock, 0o755, 1, 1).with_shared_pages(Arc::new(SharedHost));
            let (kernel, task, _) = spawn_on(
                Kernel::new(host, Arc::new(root)),
                Arc::new(Input(Mutex::new(Vec::new()))),
                &[],
                &[],
            );
            let memory = MappedMemory::new(2, true);
            let caller = Caller::new(task, &memory);
            let mut open = OpenFlags::O_CREAT | OpenFlags::O_RDWR;
            if noatime {
                open |= OpenFlags::O_NOATIME;
            }
            let fd = super::mapping::open(&kernel, &caller, b"/mapped", open);
            ftruncate(&kernel, &caller, fd, i64::from(PAGE));
            let file = kernel.file(task, fd).unwrap();
            let node = file.inode().unwrap().node();
            let old = Timestamp::new(1, 0);
            block_on(node.setattr(&SetAttr {
                atime: Some(TimeSet::At(old)),
                ..SetAttr::default()
            }))
            .unwrap();
            assert_eq!(
                errno(block_on(kernel.mmap(&caller, 0, 0, RW, flags, fd, 0))),
                Errno::EINVAL
            );
            assert_eq!(
                errno(block_on(kernel.mmap(
                    &caller,
                    1,
                    PAGE,
                    RW,
                    flags | MapFlags::MAP_FIXED,
                    fd,
                    0
                ))),
                Errno::EINVAL
            );
            assert_eq!(block_on(node.getattr()).unwrap().atime, old);
            let at = block_on(kernel.mmap(&caller, 0, PAGE, RW, flags, fd, 0)).unwrap();
            let expected = if noatime {
                old
            } else {
                Timestamp::new(1_700_000_000, 0)
            };
            assert_eq!(block_on(node.getattr()).unwrap().atime, expected);
            block_on(kernel.munmap(&caller, at, PAGE)).unwrap();
        }
    }
}

/// POSIX mmap timestamp window: a writable shared mapping is marked by msync
/// or removal, even when a same-value store cannot be detected from its bytes.
#[test]
fn shared_file_mapping_marks_write_times_at_sync_and_removal() {
    use wasixv2_vfs::{SetAttr, TimeSet, Timestamp};
    for native in [false, true] {
        for operation in [
            "sync",
            "async",
            "unmap",
            "exit",
            "readonly",
            "private",
            "invalidate",
            "empty",
            "protect",
        ] {
            let (kernel, task) = shared_machine();
            let memory: Box<dyn UserMemory> = if native {
                Box::new(MappedMemory::new(2, true))
            } else {
                Box::new(FlatMemory(RefCell::new(vec![0; 2 * PAGE as usize])))
            };
            let caller = Caller::new(task, &*memory);
            let fd = memfd(&kernel, &caller, MemfdFlags::empty());
            ftruncate(&kernel, &caller, fd, i64::from(2 * PAGE));
            let prot = if operation == "readonly" {
                ProtFlags::PROT_READ
            } else {
                RW
            };
            let flags = if operation == "private" {
                MapFlags::MAP_PRIVATE
            } else {
                MapFlags::MAP_SHARED
            };
            let at = block_on(kernel.mmap(&caller, 0, 2 * PAGE, prot, flags, fd, 0)).unwrap();
            let file = kernel.file(task, fd).unwrap();
            let node = file.inode().unwrap().node();
            let old = Timestamp::new(1, 0);
            let reset = || {
                block_on(node.setattr(&SetAttr {
                    mtime: Some(TimeSet::At(old)),
                    ctime: Some(TimeSet::At(old)),
                    ..SetAttr::default()
                }))
                .unwrap();
            };
            let check = |marked| {
                let attr = block_on(node.getattr()).unwrap();
                let expected = if marked {
                    Timestamp::new(1_700_000_000, 0)
                } else {
                    old
                };
                assert_eq!(
                    (attr.mtime, attr.ctime),
                    (expected, expected),
                    "{operation} native={native}"
                );
            };
            reset();
            // No content change: the approved conservative mark cannot rely on a diff.
            match operation {
                "unmap" => {
                    block_on(kernel.munmap(&caller, at, 2 * PAGE)).unwrap();
                }
                "exit" => {
                    block_on(kernel.exit_group(&caller, 0)).unwrap();
                }
                "empty" => {
                    block_on(kernel.msync(&caller, at + PAGE, 0, MsyncFlags::MS_SYNC)).unwrap();
                }
                "protect" => {
                    // Partial downgrade must preserve the first page's pending mark only
                    // until it has been accounted, without losing the writable second page.
                    block_on(kernel.mprotect(&caller, at, PAGE, ProtFlags::PROT_READ)).unwrap();
                    block_on(kernel.msync(&caller, at, PAGE, MsyncFlags::MS_SYNC)).unwrap();
                    check(true);
                    reset();
                    block_on(kernel.msync(&caller, at, PAGE, MsyncFlags::MS_SYNC)).unwrap();
                    check(false);
                    block_on(kernel.msync(&caller, at + PAGE, PAGE, MsyncFlags::MS_ASYNC)).unwrap();
                    check(true);
                    reset();
                    block_on(kernel.mprotect(&caller, at, PAGE, RW)).unwrap();
                    block_on(kernel.msync(&caller, at, PAGE, MsyncFlags::MS_SYNC)).unwrap();
                }
                _ => {
                    let flags = match operation {
                        "async" => MsyncFlags::MS_ASYNC,
                        "invalidate" => MsyncFlags::MS_INVALIDATE,
                        _ => MsyncFlags::MS_SYNC,
                    };
                    block_on(kernel.msync(&caller, at, 2 * PAGE, flags)).unwrap();
                }
            }
            check(!matches!(
                operation,
                "readonly" | "private" | "invalidate" | "empty"
            ));
            if matches!(operation, "sync" | "async") {
                reset();
                let flags = if operation == "sync" {
                    MsyncFlags::MS_SYNC
                } else {
                    MsyncFlags::MS_ASYNC
                };
                block_on(kernel.msync(&caller, at, 2 * PAGE, flags)).unwrap();
                check(true);
            }
        }
    }
}
