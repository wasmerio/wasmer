//! Shared objects whose pages the memory shows itself: `MAP_SHARED | MAP_ANONYMOUS` on a host
//! that has shared memory, over memory that can place an object's pages, as the execution
//! layer's on Unix.

use std::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
};

use wasixv2_abi::{MadviseAdvice, MapFlags, MremapFlags, ProtFlags};
use wasixv2_pages::{Memfd, SharedPages, map_at, map_private_at};

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
