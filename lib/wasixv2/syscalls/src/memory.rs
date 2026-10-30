use std::{
    future::Future,
    ops::Range,
    pin::Pin,
    sync::{
        PoisonError, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use wasixv2_abi::{Errno, GuestLayout, GuestPtr, GuestPtrMut};
use wasixv2_pages::SharedPages;

/// Boxed future without a `Send` bound; wasmer's async host calls are not `Send`.
pub type LocalFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A guest address outside of mapped memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault;

/// The future of a copy that already happened, as [`UserMemory::read`] and
/// [`UserMemory::write`] return it. Both outcomes are zero-sized futures, whose box allocates
/// nothing, so a memory that can copy without waiting costs no allocation per copy.
pub fn copied(result: Result<(), Fault>) -> LocalFuture<'static, Result<(), Fault>> {
    match result {
        Ok(()) => Box::pin(Copied::<true>),
        Err(Fault) => Box::pin(Copied::<false>),
    }
}

/// A finished copy: `OK` or a [`Fault`].
struct Copied<const OK: bool>;

impl<const OK: bool> Future for Copied<OK> {
    type Output = Result<(), Fault>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(if OK { Ok(()) } else { Err(Fault) })
    }
}

/// Immutable image metadata used to choose Linux's brk and mmap layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryLayout {
    /// End of the image's occupied bytes, including the main stack and linker reservations.
    pub image_end: u64,
    /// The data segment, including BSS, for Linux's `check_data_rlimit` accounting.
    pub data: Range<u64>,
    /// Initial program break, at or above `image_end`.
    pub brk: u64,
    /// Exclusive linear-memory maximum, at most 2^32 and aligned to a wasm page.
    pub maximum: u64,
    /// Page-aligned execution-layer reservations, excluded from brk and mmap allocation.
    /// They may be backed lazily, but must remain reserved for the lifetime of the image.
    pub reserved: Vec<Range<u64>>,
}

/// The calling task's address space, read and written the way Linux's `copy_{from,to}_user` do:
/// a copy that touches the low guard or a page its address space's [`PageAccess`] keeps from
/// it faults. This is independent of the guest's own loads, which may read the low guard.
pub trait UserMemory {
    /// Immutable image layout for Linux's separate brk and top-down mmap arenas. The image,
    /// including its main stack, and `reserved` ranges remain owned by the execution layer;
    /// other pages above the image are available for kernel mappings even if linear memory
    /// already backs them. Growing linear memory alone does not map those pages.
    ///
    /// `None` preserves the original allocation contract: everything initially in memory
    /// belongs to the image, and new mappings grow memory or reuse pages previously freed.
    /// Copies for a fork child must report the same layout as the parent.
    fn layout(&self) -> LocalFuture<'_, Option<MemoryLayout>> {
        Box::pin(async { None })
    }

    fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>>;
    fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>>;

    /// [`UserMemory::read`] whatever the pages' protection, as the kernel reads a mapping's
    /// pages on its own behalf (`FOLL_FORCE`): to write a file mapping back, to move pages.
    /// Only what lies outside the memory faults.
    fn force_read<'a>(
        &'a self,
        addr: u32,
        buf: &'a mut [u8],
    ) -> LocalFuture<'a, Result<(), Fault>> {
        self.read(addr, buf)
    }

    /// [`UserMemory::write`] whatever the pages' protection (`FOLL_FORCE`): to fill a mapping
    /// with its file, to move pages. What the host itself cannot write (the low guard, a
    /// read-only view of a shared object) still faults.
    fn force_write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
        self.write(addr, data)
    }
    /// Size of the address space in bytes.
    fn size(&self) -> LocalFuture<'_, u64>;
    /// Grows the address space by `pages` wasm pages of 64 KiB and returns its previous size in
    /// bytes, or `None` if it cannot grow that far.
    fn grow(&self, pages: u32) -> LocalFuture<'_, Option<u64>>;
    /// Whether the address space can be duplicated for a child that does not share it, as
    /// `clone` without `CLONE_VM` does (`dup_mm`). Wasm has no copy-on-write and keeps the call
    /// stack outside linear memory, so only an engine that can capture the caller's stack says
    /// yes: the asyncify build configuration, called from the guest's entry point.
    fn can_duplicate(&self) -> LocalFuture<'_, bool> {
        Box::pin(async { false })
    }

    /// The calling task's stack pointer as the call found it (`current_user_stack_pointer`):
    /// the guest's shadow stack pointer, which tells whether it runs on its alternate signal
    /// stack. 0 for memory no guest call runs on.
    fn stack_pointer(&self) -> LocalFuture<'_, u32> {
        Box::pin(async { 0 })
    }

    /// The main thread's stack as the image laid it out: the bytes from the bottom of the
    /// region the linker reserved for it up to where its `__stack_pointer` starts. Empty where
    /// it is not known. The kernel accounts only the part a program used, as Linux's stack area
    /// grows (DESIGN.md, "Locked memory").
    fn main_stack(&self) -> LocalFuture<'_, Range<u32>> {
        Box::pin(async { 0..0 })
    }

    /// Bytes at the bottom of the address space the host keeps as the low guard, 0 for none:
    /// they stand for the pages below `mmap_min_addr` that Linux never maps. Ordinary reads
    /// and writes there fail with [`Fault`]. The guest's own loads can still read zeros there
    /// for LLVM's null-va_list compatibility (DESIGN.md, "Guard page at 0").
    fn low_guard(&self) -> LocalFuture<'_, u32> {
        Box::pin(async { 0 })
    }

    /// Makes `[addr, addr + len)`, whole pages inside the memory and above any guard, show `len`
    /// bytes of `pages` from `offset`, the same pages every other mapping of the object shows
    /// (`MAP_SHARED`): from then on a store by any process that maps them is seen by all at
    /// once. What the range held is gone. `pages` must be at least `offset + len` bytes long for
    /// as long as the range shows it.
    ///
    /// Without `writable` the range is a read-only view of the object (a `SHM_RDONLY`
    /// attachment, a shared mapping of a file not open for writing): it still shows every
    /// other process's stores, but a store by the guest into it faults (`SIGSEGV`), and
    /// [`UserMemory::write`] and every other write into it fail with [`Fault`], until the range
    /// is mapped again.
    ///
    /// `EOPNOTSUPP` from memory that cannot share pages with other memories, which is the
    /// default: wasm alone cannot, and only a host that places pages inside a linear memory
    /// can. The kernel then keeps copies instead, synchronised at defined points.
    fn map_shared<'a>(
        &'a self,
        addr: u32,
        len: u64,
        pages: &'a dyn SharedPages,
        offset: u64,
        writable: bool,
    ) -> LocalFuture<'a, Result<(), Errno>> {
        let _ = (addr, len, pages, offset, writable);
        Box::pin(async { Err(Errno::EOPNOTSUPP) })
    }

    /// Ends what [`UserMemory::map_shared`] made of `[addr, addr + len)`: the range holds
    /// private zeroed pages again, as fresh anonymous memory reads, writable whether or not the
    /// object's were.
    fn unmap_shared(&self, addr: u32, len: u64) -> LocalFuture<'_, Result<(), Errno>> {
        let _ = (addr, len);
        Box::pin(async { Err(Errno::EOPNOTSUPP) })
    }

    /// Makes `[addr, addr + len)`, whole pages of the memory's own private memory (no shared
    /// object shows there), read zero, as fresh anonymous pages do, whatever their protection
    /// (the kernel's own fill, as [`UserMemory::force_write`]). Fails with [`Fault`] where
    /// [`UserMemory::force_write`] would.
    ///
    /// The default writes zeros. Memory that can give the range fresh pages instead does
    /// (`MADV_DONTNEED` on a Linux host), so pages no one wrote never take host memory: a
    /// mapping that reuses unmapped pages costs no more than one that grows memory.
    fn zero_pages(&self, addr: u32, len: u64) -> LocalFuture<'_, Result<(), Fault>> {
        Box::pin(async move {
            let zeroes = vec![0; WASM_PAGE_SIZE as usize];
            let end = u64::from(addr) + len;
            let mut pos = u64::from(addr);
            while pos < end {
                let n = (end - pos).min(WASM_PAGE_SIZE);
                let at = u32::try_from(pos).map_err(|_| Fault)?;
                self.force_write(at, &zeroes[..n as usize]).await?;
                pos += n;
            }
            Ok(())
        })
    }

    /// Replaces the `u32` at `addr`, which is 4-byte aligned, with `new` if it holds `expected`,
    /// and returns what it held (`futex_atomic_cmpxchg_inatomic`). The futex code relies on
    /// this to change a futex word under the feet of guest threads.
    ///
    /// Memory that other threads use at the same time must override this with an atomic
    /// compare-and-exchange. The default reads and then writes, which is only right when
    /// nothing else touches the memory meanwhile.
    fn compare_exchange_u32(
        &self,
        addr: u32,
        expected: u32,
        new: u32,
    ) -> LocalFuture<'_, Result<u32, Fault>> {
        Box::pin(async move {
            let mut word = [0; 4];
            self.read(addr, &mut word).await?;
            let current = u32::from_le_bytes(word);
            if current == expected {
                self.write(addr, &new.to_le_bytes()).await?;
            }
            Ok(current)
        })
    }
}

/// What the kernel's copies may do with the pages of an address space: the page map's verdict,
/// which the kernel keeps up to date and every [`UserMemory`] of the address space checks
/// before a copy, as Linux's `copy_from_user` and `copy_to_user` fault on a page that is not
/// mapped or whose protection forbids the access. Only the ranges the kernel knows to be
/// restricted are recorded: pages it never handed out, such as memory the guest grew itself,
/// are readable and writable. The guest's own loads and stores are not checked, as wasm has no
/// page protection.
#[derive(Debug, Default)]
pub struct PageAccess {
    /// Whether any range is restricted, so the common case takes no lock.
    restricted: AtomicBool,
    /// The restricted ranges, lowest first and apart.
    ranges: RwLock<Vec<Restricted>>,
}

/// A run of pages with less than read and write access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restricted {
    pub range: Range<u64>,
    /// Whether reads may go there: a mapping without `PROT_WRITE`. Otherwise nothing may (not
    /// mapped, or `PROT_NONE`).
    pub readable: bool,
}

impl PageAccess {
    /// Replaces what is restricted with `ranges`, lowest first and apart.
    pub fn set(&self, ranges: Vec<Restricted>) {
        let mut held = self.ranges.write().unwrap_or_else(PoisonError::into_inner);
        self.restricted.store(!ranges.is_empty(), Ordering::Release);
        *held = ranges;
    }

    /// Replaces what is restricted in `[start, end)` with `ranges`, which lie inside it, lowest
    /// first and apart, keeping the rest: the change of one part of the page map, as Linux
    /// changes the page tables of the pages an `mprotect` or `munmap` covers only. Ranges that
    /// touch and allow the same join, as [`PageAccess::set`] would have them.
    pub fn update(&self, start: u64, end: u64, ranges: Vec<Restricted>) {
        let mut held = self.ranges.write().unwrap_or_else(PoisonError::into_inner);
        let first = held.partition_point(|r| r.range.end <= start);
        let last = held.partition_point(|r| r.range.start < end);
        let mut middle = Vec::with_capacity(ranges.len() + 2);
        if first < last {
            let head = &held[first];
            if head.range.start < start {
                middle.push(Restricted {
                    range: head.range.start..start,
                    readable: head.readable,
                });
            }
        }
        let tail = held[first..last]
            .last()
            .filter(|tail| tail.range.end > end)
            .map(|tail| Restricted {
                range: end..tail.range.end,
                readable: tail.readable,
            });
        middle.extend(ranges);
        middle.extend(tail);
        let changed = middle.len();
        held.splice(first..last, middle);
        // Join across the edges of the change.
        let mut at = first.saturating_sub(1);
        let mut stop = (first + changed + 1).min(held.len());
        while at + 1 < stop {
            let (left, right) = (&held[at], &held[at + 1]);
            if left.range.end == right.range.start && left.readable == right.readable {
                held[at].range.end = held[at + 1].range.end;
                held.remove(at + 1);
                stop -= 1;
            } else {
                at += 1;
            }
        }
        self.restricted.store(!held.is_empty(), Ordering::Release);
    }

    /// The restricted ranges, lowest first.
    pub fn ranges(&self) -> Vec<Restricted> {
        self.ranges
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether `len` bytes at `addr` may be read, or written with `write`. An empty access
    /// touches nothing, as a copy of 0 bytes never faults. Inlined into every copy: while
    /// nothing is restricted, it is one load.
    #[inline]
    pub fn check(&self, addr: u32, len: usize, write: bool) -> Result<(), Fault> {
        if len == 0 || !self.restricted.load(Ordering::Acquire) {
            return Ok(());
        }
        self.check_ranges(addr, len, write)
    }

    /// [`PageAccess::check`] against the restricted ranges.
    #[cold]
    #[inline(never)]
    fn check_ranges(&self, addr: u32, len: usize, write: bool) -> Result<(), Fault> {
        let (start, end) = (u64::from(addr), u64::from(addr) + len as u64);
        let ranges = self.ranges.read().unwrap_or_else(PoisonError::into_inner);
        let first = ranges.partition_point(|r| r.range.end <= start);
        let denied = ranges[first..]
            .iter()
            .take_while(|r| r.range.start < end)
            .any(|r| write || !r.readable);
        match denied {
            true => Err(Fault),
            false => Ok(()),
        }
    }
}

/// Bytes of a wasm page, the unit `UserMemory::grow` counts in.
pub const WASM_PAGE_SIZE: u64 = 64 * 1024;

/// A C string longer than the limit the caller gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CstrError {
    Fault,
    TooLong,
}

impl From<Fault> for CstrError {
    fn from(_: Fault) -> Self {
        Self::Fault
    }
}

/// Reads the value `ptr` points to.
pub async fn read_value<T: GuestLayout>(
    memory: &dyn UserMemory,
    ptr: GuestPtr<T>,
) -> Result<T, Fault> {
    let mut scratch = Scratch::new(T::SIZE);
    let buf = scratch.get();
    memory.read(ptr.addr(), buf).await?;
    T::read_from(buf).ok_or(Fault)
}

/// Writes `value` where `ptr` points.
pub async fn write_value<T: GuestLayout>(
    memory: &dyn UserMemory,
    ptr: GuestPtrMut<T>,
    value: &T,
) -> Result<(), Fault> {
    let mut scratch = Scratch::new(T::SIZE);
    let buf = scratch.get();
    value.write_to(buf);
    memory.write(ptr.addr(), buf).await
}

/// A buffer for one guest value: on the stack for the scalars and small structs nearly every
/// copy is about, on the heap for the few larger ones.
enum Scratch {
    Stack([u8; 128], usize),
    Heap(Vec<u8>),
}

impl Scratch {
    fn new(size: usize) -> Self {
        match size <= 128 {
            true => Self::Stack([0; 128], size),
            false => Self::Heap(vec![0; size]),
        }
    }

    fn get(&mut self) -> &mut [u8] {
        match self {
            Self::Stack(bytes, size) => &mut bytes[..*size],
            Self::Heap(bytes) => bytes,
        }
    }
}

/// Reads the NUL-terminated string at `ptr`, without the NUL, failing with `TooLong` unless the
/// NUL is within `max` bytes, as Linux's `strncpy_from_user` does for `PATH_MAX`.
pub async fn read_cstr(
    memory: &dyn UserMemory,
    ptr: GuestPtr<u8>,
    max: usize,
) -> Result<Vec<u8>, CstrError> {
    // Aligned chunks never straddle the end of memory, which is a multiple of the chunk size.
    const CHUNK: u32 = 256;
    let mut out = Vec::new();
    let mut addr = ptr.addr();
    while out.len() < max {
        let len = CHUNK - addr % CHUNK;
        let mut chunk = vec![0; len as usize];
        memory.read(addr, &mut chunk).await?;
        if let Some(nul) = chunk.iter().position(|&b| b == 0) {
            out.extend_from_slice(&chunk[..nul]);
            return if out.len() < max {
                Ok(out)
            } else {
                Err(CstrError::TooLong)
            };
        }
        out.extend_from_slice(&chunk);
        addr = addr.checked_add(len).ok_or(Fault)?;
    }
    Err(CstrError::TooLong)
}

/// A guest buffer the kernel reads from. Nothing is copied until the kernel asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserSlice {
    pub addr: u32,
    pub len: u32,
}

impl UserSlice {
    pub fn new(addr: u32, len: u32) -> Self {
        Self { addr, len }
    }

    /// Copies at most `max` bytes in from the guest.
    pub async fn copy_in(self, memory: &dyn UserMemory, max: usize) -> Result<Vec<u8>, Fault> {
        let mut buf = vec![0; (self.len as usize).min(max)];
        memory.read(self.addr, &mut buf).await?;
        Ok(buf)
    }
}

/// A guest buffer the kernel writes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserSliceMut {
    pub addr: u32,
    pub len: u32,
}

impl UserSliceMut {
    pub fn new(addr: u32, len: u32) -> Self {
        Self { addr, len }
    }

    /// Copies `data` out to the start of the buffer. More than fits faults rather than write
    /// past the buffer.
    pub async fn copy_out(self, memory: &dyn UserMemory, data: &[u8]) -> Result<(), Fault> {
        if data.len() > self.len as usize {
            return Err(Fault);
        }
        memory.write(self.addr, data).await
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Flat(RefCell<Vec<u8>>);

    impl UserMemory for Flat {
        fn read<'a>(&'a self, addr: u32, buf: &'a mut [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            let memory = self.0.borrow();
            let result = memory
                .get(addr as usize..addr as usize + buf.len())
                .map(|src| buf.copy_from_slice(src))
                .ok_or(Fault);
            Box::pin(async move { result })
        }

        fn write<'a>(&'a self, addr: u32, data: &'a [u8]) -> LocalFuture<'a, Result<(), Fault>> {
            let mut memory = self.0.borrow_mut();
            let result = memory
                .get_mut(addr as usize..addr as usize + data.len())
                .map(|dst| dst.copy_from_slice(data))
                .ok_or(Fault);
            Box::pin(async move { result })
        }

        fn size(&self) -> LocalFuture<'_, u64> {
            let size = self.0.borrow().len() as u64;
            Box::pin(async move { size })
        }

        fn grow(&self, _: u32) -> LocalFuture<'_, Option<u64>> {
            Box::pin(async { None })
        }
    }

    #[test]
    fn memory_without_layout_preserves_the_original_contract() {
        let memory = Flat(RefCell::new(vec![0; 8]));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert_eq!(
            memory.layout().as_mut().poll(&mut cx),
            std::task::Poll::Ready(None)
        );
    }

    #[test]
    fn copy_out_faults_instead_of_writing_past_the_buffer() {
        let memory = Flat(RefCell::new(vec![0; 8]));
        let buf = UserSliceMut::new(0, 2);
        let copy = |data: &[u8]| {
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            std::pin::pin!(buf.copy_out(&memory, data)).poll(&mut cx)
        };
        assert_eq!(copy(b"abc"), std::task::Poll::Ready(Err(Fault)));
        assert_eq!(*memory.0.borrow(), [0; 8]);
        assert_eq!(copy(b"ab"), std::task::Poll::Ready(Ok(())));
        assert_eq!(memory.0.borrow()[..3], *b"ab\0");
    }

    /// A finished copy reports its outcome, and its future is zero-sized: boxing it allocates
    /// nothing.
    #[test]
    fn page_access_faults_where_the_page_map_says() {
        let access = PageAccess::default();
        assert_eq!(access.check(0, 1 << 20, true), Ok(()), "nothing restricted");
        access.set(vec![
            Restricted {
                range: 0x1_0000..0x2_0000,
                readable: false,
            },
            Restricted {
                range: 0x3_0000..0x4_0000,
                readable: true,
            },
        ]);
        assert_eq!(
            access.check(0xff00, 0x100, true),
            Ok(()),
            "ends where it starts"
        );
        assert_eq!(access.check(0xff00, 0x101, false), Err(Fault));
        assert_eq!(access.check(0x1_8000, 0, false), Ok(()), "an empty copy");
        assert_eq!(access.check(0x2_0000, 0x1_0000, true), Ok(()));
        assert_eq!(access.check(0x3_0000, 8, false), Ok(()), "read-only reads");
        assert_eq!(
            access.check(0x3_fff8, 16, true),
            Err(Fault),
            "read-only writes"
        );
        assert_eq!(
            access.check(0x1000, 0x10_0000, false),
            Err(Fault),
            "spanning"
        );
        access.set(Vec::new());
        assert_eq!(access.check(0x1_0000, 8, true), Ok(()));
    }

    #[test]
    fn copied_needs_no_allocation() {
        assert_eq!(size_of::<Copied<true>>(), 0);
        assert_eq!(size_of::<Copied<false>>(), 0);
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert_eq!(copied(Ok(())).as_mut().poll(&mut cx), Poll::Ready(Ok(())));
        assert_eq!(
            copied(Err(Fault)).as_mut().poll(&mut cx),
            Poll::Ready(Err(Fault))
        );
    }

    fn r(range: Range<u64>, readable: bool) -> Restricted {
        Restricted { range, readable }
    }

    /// An update replaces only its own part, clips what straddles it, and joins what touches.
    #[test]
    fn an_update_changes_its_part_of_the_access_only() {
        let access = PageAccess::default();
        access.set(vec![r(0..10, false), r(20..40, true), r(50..60, false)]);
        access.update(30, 55, vec![r(30..35, true), r(45..55, false)]);
        assert_eq!(
            access.ranges(),
            [r(0..10, false), r(20..35, true), r(45..60, false)]
        );
        access.update(0, 100, Vec::new());
        assert!(access.ranges().is_empty());
        assert_eq!(access.check(0, 4, true), Ok(()));
    }
}
