//! Shared objects whose pages the memory shows itself: `MAP_SHARED | MAP_ANONYMOUS` on a host
//! that has shared memory, over memory that can place an object's pages, as the execution
//! layer's on Unix.

use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

use futures::FutureExt as _;
use wasixv2_abi::{FutexOp, MadviseAdvice, MapFlags, MremapFlags, ProtFlags};
use wasixv2_pages::{Memfd, SharedPages, map_at, map_private_at};
use wasixv2_syscalls::{AtomicWait, AtomicWord};

use super::{ipc::errno, *};

const PAGE: u32 = WASM_PAGE_SIZE as u32;
const RW: ProtFlags = ProtFlags(3);
/// Pages a [`MappedMemory`] may grow to.
const RESERVED: usize = 64;

/// Linear memory the way the execution layer has it on Unix: one host mapping reserved for all
/// it may grow to, whose ranges can show a shared object's pages.
pub(super) struct MappedMemory {
    base: *mut u8,
    size: Cell<usize>,
    forkable: bool,
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
        }
    }

    /// A fork child's memory: a copy of this one, which shows `shared` again as the execution
    /// layer's copy does.
    pub(super) fn fork(&self, shared: &[wasixv2_syscalls::SharedMapping]) -> Self {
        let child = Self::new(self.size.get() / PAGE as usize, self.forkable);
        // SAFETY: both ranges lie inside their reservations, which do not overlap.
        unsafe { std::ptr::copy_nonoverlapping(self.base, child.base, self.size.get()) };
        for mapping in shared {
            block_on(child.map_shared(mapping.addr, mapping.len, &*mapping.pages, mapping.offset))
                .unwrap();
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
        let result = self.range(addr, data.len()).map(|dst| {
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
        let result = self.range(addr, 4).map(|ptr| {
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
    ) -> LocalFuture<'a, Result<(), Errno>> {
        let result = self.range(addr, len as usize).map_err(|_| Errno::EINVAL);
        // SAFETY: inside the reservation this memory owns.
        let result = result.and_then(|ptr| {
            unsafe { map_at(ptr, len as usize, pages, offset) }.map_err(|_| Errno::EINVAL)
        });
        Box::pin(async move { result })
    }

    fn unmap_shared(&self, addr: u32, len: u64) -> LocalFuture<'_, Result<(), Errno>> {
        let result = self.range(addr, len as usize).map_err(|_| Errno::EINVAL);
        // SAFETY: inside the reservation this memory owns.
        let result = result.and_then(|ptr| {
            unsafe { map_private_at(ptr, len as usize) }.map_err(|_| Errno::EINVAL)
        });
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

/// A kernel on [`SharedHost`] with one task.
pub(super) fn shared_machine() -> (Kernel, TaskId) {
    let host: Arc<dyn Host> = Arc::new(SharedHost);
    let clock = Arc::new(HostClock(host.clone()));
    let root = MemFs::new(clock, 0o755, 1, 1);
    let kernel = Kernel::new(host, Arc::new(root));
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
