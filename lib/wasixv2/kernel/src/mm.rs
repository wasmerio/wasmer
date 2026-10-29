//! The program break, anonymous and file mappings inside the guest's linear memory.
//!
//! Linear memory only grows, so `brk` and `mmap` hand out pages above everything the image owns
//! (its data, stack and heap) and grow memory when no unmapped run fits. `munmap` returns pages
//! for reuse by later mappings, which are zeroed then, by dropping them where the memory can
//! ([`UserMemory::zero_pages`]), so a page no one wrote takes no host memory however often it is
//! mapped, as on Linux, where even a large `PROT_NONE` mapping costs nothing until it is
//! touched. Wasm has no page protection: the guest's own loads and stores reach unmapped and
//! protected pages alike. The kernel's copies do not: the page map and the protections `mmap`
//! and `mprotect` set are published in the address space's [`PageAccess`], which every copy
//! checks, so a syscall faults on them as on Linux (DESIGN.md, "Page protection"). Executable
//! memory is refused, since wasm cannot run code generated at run time.
//!
//! File mappings and SysV segment attachments are copies kept coherent at defined points (see
//! [`shared`]). A shared file mapping is synchronised with its file:
//!
//! - by `msync` over it, and `MADV_DONTNEED`;
//! - by `munmap`, `mremap` and `MAP_FIXED` over it, and when the address space ends with
//!   `exit`, `exit_group` or `execve` (written back only);
//! - around every `read`, `write` and other data syscall the same process makes on the file,
//!   so within one process `read` sees what the mapping wrote and the mapping sees what
//!   `write` wrote, as on Linux.
//!
//! Another process sees a change once both sides passed such a point. A process killed by a
//! signal writes nothing back: its memory is gone before the kernel could read it.
//!
//! `MAP_SHARED | MAP_ANONYMOUS` memory lives in a shared object of the host's
//! ([`Host::shared_pages`]) whose pages the memory itself shows ([`UserMemory::map_shared`]), so
//! a fork child and its parent see each other's stores at once, as on Linux. Where the host or
//! the memory cannot do that, it is ordinary memory of the address space, which a fork child
//! gets a copy of. A shared mapping of a file that its filesystem keeps in such an object
//! (memfd and `/dev/shm` files, [`wasixv2_vfs::Node::shared_pages`]) shows the object the same
//! way, read-only if it may never be written, so it needs none of the synchronisation above:
//! `read` and `write` reach the very pages every mapping shows. Where the memory cannot show
//! them, it is a copy as above.
//!
//! [`Host::shared_pages`]: wasixv2_host::Host::shared_pages

mod shared;

use std::{
    io,
    ops::{Deref, DerefMut, Range},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

pub(crate) use shared::{Mapped, Mapping, Object, Shared, Source, Writer};
use wasixv2_abi::{
    Buf, Fd, Loff, MadviseAdvice, MapFlags, MclFlags, MlockFlags, MremapFlags, MsyncFlags,
    OpenFlags, ProtFlags, Ulong, Uptr, Usize,
};
use wasixv2_syscalls::{
    Caller, Errno, PageAccess, Restricted, SharedMapping, SysResult, TaskId, UserMemory,
    WASM_PAGE_SIZE,
};
use wasixv2_vfs::{Cap, FileType, WRITE_SEALS};

use crate::{Kernel, file::OpenFile, sync::MutexExt};

/// The page size the guest sees in `AT_PAGESZ` and the unit of every mapping.
pub(crate) const PAGE_SIZE: u64 = WASM_PAGE_SIZE;

/// `mmap_min_addr`: no mapping may start below it, so a null pointer dereference cannot reach
/// mapped memory. The execution layer makes the memory below it read-only where the program's
/// layout allows. Linux's default on x86 and arm64 (`mm/Kconfig`, `DEFAULT_MMAP_MIN_ADDR`;
/// `security/Kconfig`, `LSM_MMAP_MIN_ADDR`), and like the LSM minimum
/// (`security/min_addr.c`) no capability lowers it.
pub(crate) const MMAP_MIN_ADDR: u64 = 65536;

/// `LEGACY_MAP_MASK`: the flags `MAP_SHARED_VALIDATE` accepts for a file.
const LEGACY_MAP_MASK: MapFlags = MapFlags(
    MapFlags::MAP_SHARED.0
        | MapFlags::MAP_PRIVATE.0
        | MapFlags::MAP_FIXED.0
        | MapFlags::MAP_ANONYMOUS.0
        | MapFlags::MAP_DENYWRITE.0
        | MapFlags::MAP_EXECUTABLE.0
        | MapFlags::MAP_UNINITIALIZED.0
        | MapFlags::MAP_GROWSDOWN.0
        | MapFlags::MAP_LOCKED.0
        | MapFlags::MAP_NORESERVE.0
        | MapFlags::MAP_POPULATE.0
        | MapFlags::MAP_NONBLOCK.0
        | MapFlags::MAP_STACK.0
        | MapFlags::MAP_HUGETLB.0,
);

/// An address space and the lock that orders changes to it (`mm_struct`). Every task on the
/// same linear memory shares one: the threads of a group, and a vfork child while it runs in its
/// parent's instance.
pub(crate) struct Mm {
    /// Names the address space in futex keys; never reused.
    id: u64,
    /// Held by every call that reads the size of memory, grows it, or takes pages, from the
    /// first look at the page map to the last update, and by every synchronisation of a
    /// mapping. Each thread runs its own instance on the shared memory, so without it two
    /// threads interleave their growths and each other's checks.
    op: futures_util::lock::Mutex<()>,
    space: Mutex<AddressSpace>,
    /// What the kernel's copies may do with each page, as `space` says: every change to the
    /// page map reaches it ([`Space`]), and the execution layer checks every copy against it.
    access: Arc<PageAccess>,
    /// Whether any shared mapping exists, so file I/O skips the mapping table otherwise.
    shared: AtomicBool,
}

/// The page map, locked. A change through it reaches the address space's [`PageAccess`] when
/// it is let go, still under the lock, so the two never disagree for a copy that comes after.
pub(crate) struct Space<'a> {
    space: MutexGuard<'a, AddressSpace>,
    access: &'a PageAccess,
    changed: bool,
}

impl Deref for Space<'_> {
    type Target = AddressSpace;

    fn deref(&self) -> &AddressSpace {
        &self.space
    }
}

impl DerefMut for Space<'_> {
    fn deref_mut(&mut self) -> &mut AddressSpace {
        self.changed = true;
        &mut self.space
    }
}

impl Drop for Space<'_> {
    fn drop(&mut self) {
        if self.changed {
            self.access.set(self.space.restricted());
        }
    }
}

impl Default for Mm {
    fn default() -> Self {
        Self::with_space(AddressSpace::default())
    }
}

impl Mm {
    fn with_space(space: AddressSpace) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let access = PageAccess::default();
        access.set(space.restricted());
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            op: Default::default(),
            shared: AtomicBool::new(space.mappings.any_shared()),
            space: Mutex::new(space),
            access: Arc::new(access),
        }
    }

    /// What the kernel's copies may do with each page of the address space.
    pub(crate) fn access(&self) -> &Arc<PageAccess> {
        &self.access
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// `dup_mmap`: a new address space with the same mappings, for the child `pid` that copies
    /// memory. Its file mappings and segment attachments are its own from then on.
    pub(crate) fn dup(&self, pid: u32) -> Self {
        Self::with_space(self.space().dup(pid))
    }

    /// The page map. A panic elsewhere while it was held must not wedge every other thread of
    /// the process, so poisoning is ignored.
    fn space(&self) -> Space<'_> {
        Space {
            space: self.space.lock_or_recover(),
            access: &self.access,
            changed: false,
        }
    }

    /// The address space as `/proc/<pid>/maps` lists it, lowest first, as far as the kernel
    /// has seen memory.
    pub(crate) fn regions(&self) -> Vec<Region> {
        self.space().regions()
    }

    /// The futex key of the word at `addr` when it lies in a mapping of a shared object: the
    /// object and the word's offset in it (`get_futex_key`'s inode-based key).
    pub(crate) fn shared_futex_key(&self, addr: u32) -> Option<crate::futex::Key> {
        let space = self.space();
        let mapping = space.mappings.containing(addr.into())?;
        let mapped = mapping.mapped()?;
        Some(crate::futex::Key::Shared {
            object: mapped.pages.id(),
            offset: mapped.offset + (u64::from(addr) - mapping.start),
        })
    }

    /// Where this address space maps byte `offset` of the shared object `object`, if it does:
    /// the lowest such address.
    pub(crate) fn address_of(&self, object: u64, offset: u64) -> Option<u32> {
        let space = self.space();
        space.mappings.mapped().find_map(|(mapping, mapped)| {
            let inside = mapped.pages.id() == object
                && (mapped.offset..mapped.offset + mapping.len).contains(&offset);
            inside.then(|| (mapping.start + offset - mapped.offset) as u32)
        })
    }

    /// The size of memory when a memory call last looked (`total_vm`).
    pub(crate) fn size(&self) -> u64 {
        self.space().seen
    }

    /// How many bytes are locked (`locked_vm`).
    pub(crate) fn locked(&self) -> u64 {
        self.space().locked_vm() * PAGE_SIZE
    }

    fn insert(&self, mapping: Mapping) {
        let mut space = self.space();
        space.mappings.insert(mapping);
        self.shared
            .store(space.mappings.any_shared(), Ordering::Release);
    }

    fn take_range(&self, start: u64, end: u64) -> Vec<Mapping> {
        let mut space = self.space();
        let taken = space.mappings.take_range(start, end);
        self.shared
            .store(space.mappings.any_shared(), Ordering::Release);
        taken
    }

    fn restore(&self, mappings: Vec<Mapping>) {
        let mut space = self.space();
        space.mappings.restore(mappings);
        self.shared
            .store(space.mappings.any_shared(), Ordering::Release);
    }
}

/// A run of pages in use, for `/proc/<pid>/maps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) kind: RegionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionKind {
    /// The image's data, stack and heap, below the allocator's pages.
    Image,
    /// The program break's pages (`[heap]`).
    Heap,
    /// Anonymous mappings, and memory the guest grew itself.
    Anon,
}

#[derive(Default)]
pub(crate) struct AddressSpace {
    /// First page under the allocator's control, fixed by the first growth. Pages below belong to
    /// the image.
    base: Option<u64>,
    /// For each page from `base` to the end of what the allocator knows: mapped or not.
    mapped: Vec<bool>,
    /// The program break: where it started and where it is.
    brk: Option<(u64, u64)>,
    /// The size of memory when a memory call last looked.
    seen: u64,
    /// Mappings of files and segments; pages not in one are anonymous.
    mappings: shared::Mappings,
    /// `VM_WIPEONFORK`: the private anonymous pages `MADV_WIPEONFORK` marked, which a fork
    /// child gets zeroed.
    wipe_on_fork: PageFlags,
    /// `VM_LOCKED`: the pages `mlock`, `mlockall` and `MAP_LOCKED` locked, which `locked_vm`
    /// counts. Memory is never paged out, so locking changes nothing else.
    locked: PageFlags,
    /// `VM_LOCKED` in `def_flags`: every new mapping is locked (`mlockall(MCL_FUTURE)`).
    lock_future: bool,
    /// The main thread's stack as Linux's stack area, from the first lock call that saw it.
    stack: Option<StackArea>,
    /// Pages mapped `PROT_NONE`: no `VM_READ`, `VM_WRITE` or `VM_EXEC`.
    no_access: PageFlags,
    /// Pages mapped readable but not `PROT_WRITE`.
    read_only: PageFlags,
}

/// `setup_arg_pages`' `stack_expand`: how far below its top a new program's stack area reaches.
const STACK_EXPAND: u64 = 128 << 10;

/// The main thread's stack as Linux's stack area. The image reserves the whole stack, as
/// `RLIMIT_STACK` bounds Linux's, but Linux's area starts as `stack_expand` below the top
/// (`setup_arg_pages`) and grows down as far as the stack was used (`expand_stack`), never
/// back. Here it grows to every stack pointer on it a lock call sees. The pages of the
/// reservation below it are not in use for `total_vm` and locking (DESIGN.md, "Locked memory").
#[derive(Clone, Copy)]
struct StackArea {
    /// The first page wholly inside the reservation.
    first: u64,
    /// The first page of the area.
    low: u64,
}

/// A flag of each page from address 0, as far as any page has it: a `vm_flags` bit of the
/// area the page lies in.
#[derive(Clone, Default)]
struct PageFlags(Vec<bool>);

impl PageFlags {
    fn get(&self, page: u64) -> bool {
        let page = usize::try_from(page).unwrap_or(usize::MAX);
        self.0.get(page).copied().unwrap_or(false)
    }

    /// Sets or clears the flag on the pages `[first, end)`.
    fn set(&mut self, first: u64, end: u64, on: bool) {
        let (first, mut end) = (first as usize, end as usize);
        if on && self.0.len() < end {
            self.0.resize(end, false);
        }
        end = end.min(self.0.len());
        if first < end {
            self.0[first..end].fill(on);
        }
        if !on {
            let last = self.0.iter().rposition(|&on| on);
            self.0.truncate(last.map_or(0, |last| last + 1));
        }
    }

    /// How many of the pages `[first, end)` have the flag.
    fn count(&self, first: u64, end: u64) -> u64 {
        let end = (end as usize).min(self.0.len());
        let first = (first as usize).min(end);
        self.0[first..end].iter().filter(|&&on| on).count() as u64
    }

    /// `mremap`: the `new_len` bytes at `to` take the flags of the `old_len` bytes at `from`,
    /// and pages past the old length that of its last page, as the area grows or is copied
    /// with its flags (`vma_expand`, `copy_vma`). The source keeps its own.
    fn remap(&mut self, from: u64, old_len: u64, to: u64, new_len: u64) {
        let from = from / PAGE_SIZE;
        let old: Vec<bool> = (from..from + old_len / PAGE_SIZE)
            .map(|page| self.get(page))
            .collect();
        let grown = old.last().copied().unwrap_or(false);
        let to = to / PAGE_SIZE;
        for (index, page) in (to..to + new_len / PAGE_SIZE).enumerate() {
            let on = old.get(index).copied().unwrap_or(grown);
            self.set(page, page + 1, on);
        }
    }

    /// The byte ranges whose pages have the flag, lowest first.
    fn ranges(&self) -> Vec<Range<u64>> {
        let mut ranges: Vec<Range<u64>> = Vec::new();
        for (page, &on) in self.0.iter().enumerate() {
            if !on {
                continue;
            }
            let (start, end) = (page as u64 * PAGE_SIZE, (page as u64 + 1) * PAGE_SIZE);
            match ranges.last_mut() {
                Some(last) if last.end == start => last.end = end,
                _ => ranges.push(start..end),
            }
        }
        ranges
    }
}

/// Where a new mapping goes.
pub(crate) enum Placement {
    /// Reuses unmapped pages at this address, which must be zeroed.
    Reuse(u64),
    /// Needs this many new pages.
    Grow(u32),
}

impl AddressSpace {
    /// A copy for the fork child `pid`.
    fn dup(&self, pid: u32) -> Self {
        Self {
            base: self.base,
            mapped: self.mapped.clone(),
            brk: self.brk,
            seen: self.seen,
            mappings: self.mappings.dup(pid),
            wipe_on_fork: self.wipe_on_fork.clone(),
            // `dup_mmap` clears `VM_LOCKED_MASK`, and `mm_init` keeps no `VM_LOCKED` of
            // `def_flags`: locks are not inherited.
            locked: PageFlags::default(),
            lock_future: false,
            stack: self.stack,
            no_access: self.no_access.clone(),
            read_only: self.read_only.clone(),
        }
    }

    /// Gives the pages `[first, end)` the protection `prot` (`vm_flags`' `VM_READ` and
    /// `VM_WRITE`): `PROT_WRITE` lets copies write, and implies reading, as on x86 and arm64;
    /// `PROT_READ` alone only reading; neither nothing.
    pub(crate) fn set_prot(&mut self, first: u64, end: u64, prot: ProtFlags) {
        let write = prot.contains(ProtFlags::PROT_WRITE);
        let read = write || prot.contains(ProtFlags::PROT_READ);
        self.no_access.set(first, end, !read);
        self.read_only.set(first, end, read && !write);
    }

    /// What the kernel's copies may not do, lowest first: nothing on the allocator's pages
    /// that are not mapped, or that are `PROT_NONE`, and no writes where `PROT_WRITE` is
    /// missing. Pages nobody handed out, which the guest grew itself, and the image's are
    /// readable and writable unless `mprotect` said otherwise.
    fn restricted(&self) -> Vec<Restricted> {
        let tracked = self.base.map_or(0, |base| base + self.mapped.len() as u64);
        let end = tracked
            .max(self.no_access.0.len() as u64)
            .max(self.read_only.0.len() as u64);
        let mut out: Vec<Restricted> = Vec::new();
        for page in 0..end {
            let mapped = match self.base {
                Some(base) if page >= base => self
                    .mapped
                    .get((page - base) as usize)
                    .copied()
                    .unwrap_or(true),
                _ => true,
            };
            let readable = match (
                mapped && !self.no_access.get(page),
                self.read_only.get(page),
            ) {
                (false, _) => false,
                (true, true) => true,
                (true, false) => continue,
            };
            let (start, stop) = (page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
            match out.last_mut() {
                Some(last) if last.range.end == start && last.readable == readable => {
                    last.range.end = stop;
                }
                _ => out.push(Restricted {
                    range: start..stop,
                    readable,
                }),
            }
        }
        out
    }

    /// Picks the first unmapped run of `pages` pages, marking it mapped.
    pub(crate) fn place(&mut self, pages: u64) -> Result<Placement, Errno> {
        if let Some(index) = self.free_run(pages) {
            self.mapped[index..][..pages as usize].fill(true);
            let base = self.base.expect("pages are tracked only after a growth");
            return Ok(Placement::Reuse((base + index as u64) * PAGE_SIZE));
        }
        let pages = u32::try_from(pages).map_err(|_| Errno::ENOMEM)?;
        Ok(Placement::Grow(pages))
    }

    /// Records that memory grew from `previous` bytes by `pages` pages for a new mapping.
    ///
    /// Growths may be recorded in any order: the guest can grow memory itself, beside the
    /// kernel. Pages someone else grew count as mapped, so they are never handed out; a growth
    /// below `base` stays the image's.
    pub(crate) fn grown(&mut self, previous: u64, pages: u32) {
        self.seen = self.seen.max(previous + u64::from(pages) * PAGE_SIZE);
        let first = previous / PAGE_SIZE;
        let base = *self.base.get_or_insert(first);
        let Some(index) = first.checked_sub(base) else {
            return;
        };
        let (index, end) = (index as usize, index as usize + pages as usize);
        if self.mapped.len() < end {
            self.mapped.resize(end, true);
        }
        self.mapped[index..end].fill(true);
    }

    /// Unmaps the pages of `[addr, addr + len)` that the allocator handed out. Whatever comes
    /// there next is a new mapping, without the flags of the old one.
    pub(crate) fn unmap(&mut self, addr: u64, len: u64) {
        let (first, end) = (addr / PAGE_SIZE, (addr + len).div_ceil(PAGE_SIZE));
        self.set_pages(first, end, false);
        self.wipe_on_fork.set(first, end, false);
        self.locked.set(first, end, false);
        self.no_access.set(first, end, false);
        self.read_only.set(first, end, false);
    }

    /// `mremap`'s flags: the `new_len` bytes at `to` take those of the `old_len` bytes at
    /// `from` (see [`PageFlags::remap`]).
    fn remap_flags(&mut self, from: u64, old_len: u64, to: u64, new_len: u64) {
        self.wipe_on_fork.remap(from, old_len, to, new_len);
        self.locked.remap(from, old_len, to, new_len);
        self.no_access.remap(from, old_len, to, new_len);
        self.read_only.remap(from, old_len, to, new_len);
    }

    /// `locked_vm`: how many pages are locked.
    fn locked_vm(&self) -> u64 {
        self.locked.count(0, u64::MAX)
    }

    /// `total_vm`: how many pages are in use below the end of memory, `mem_pages`.
    fn total_vm(&self, mem_pages: u64) -> u64 {
        (0..mem_pages)
            .filter(|&page| self.page_in_area(page, mem_pages))
            .count() as u64
    }

    /// `expand_stack` of the main stack area, the image's `stack` reservation, down to `sp`
    /// when the caller runs on it: pages the area grows by take the lock of its lowest
    /// (`acct_stack_growth`). The area starts `stack_expand` below the top.
    fn expand_stack(&mut self, stack: Range<u32>, sp: u32) {
        if stack.is_empty() {
            return;
        }
        let first = u64::from(stack.start).div_ceil(PAGE_SIZE);
        let area = *self.stack.get_or_insert(StackArea {
            first,
            low: (u64::from(stack.end).saturating_sub(STACK_EXPAND) / PAGE_SIZE).max(first),
        });
        if !stack.contains(&sp) {
            return;
        }
        let low = (u64::from(sp) / PAGE_SIZE).max(area.first);
        if low < area.low {
            let locked = self.locked.get(area.low);
            self.locked.set(low, area.low, locked);
            self.stack = Some(StackArea { low, ..area });
        }
    }

    /// `apply_mlockall_flags`: `MCL_FUTURE` locks the mappings to come, or no longer does;
    /// `MCL_CURRENT` locks every page in use, and without it every page is unlocked, unless
    /// `MCL_FUTURE` alone leaves them as they are.
    fn lock_all(&mut self, flags: MclFlags) {
        self.lock_future = flags.contains(MclFlags::MCL_FUTURE);
        if self.lock_future && !flags.contains(MclFlags::MCL_CURRENT) {
            return;
        }
        self.locked = PageFlags::default();
        if flags.contains(MclFlags::MCL_CURRENT) {
            let mem_pages = self.seen / PAGE_SIZE;
            let mut pages = 0..mem_pages;
            while let Some(first) = pages.find(|&page| self.page_in_area(page, mem_pages)) {
                let end = pages
                    .find(|&page| !self.page_in_area(page, mem_pages))
                    .unwrap_or(mem_pages);
                self.locked.set(first, end, true);
            }
        }
    }

    /// `apply_vma_lock_flags`: locks or unlocks the pages `[first, end)` in order, up to the
    /// first that is not in use (`ENOMEM`, the pages before it changed).
    fn lock_pages(
        &mut self,
        first: u64,
        end: u64,
        lock: bool,
        mem_pages: u64,
    ) -> Result<(), Errno> {
        let hole = (first..end).find(|&page| !self.page_in_area(page, mem_pages));
        self.locked.set(first, hole.unwrap_or(end), lock);
        match hole {
            Some(_) => Err(Errno::ENOMEM),
            None => Ok(()),
        }
    }

    /// Marks the allocator's pages in `[first, end)` as mapped or not.
    fn set_pages(&mut self, first: u64, end: u64, mapped: bool) {
        let Some(base) = self.base else { return };
        let end = end.min(base + self.mapped.len() as u64);
        for page in first.max(base)..end {
            self.mapped[(page - base) as usize] = mapped;
        }
    }

    /// Whether `page` lies in an area Linux would have (a `vm_area_struct`): in use, and not
    /// part of the main stack's reservation below the stack area.
    fn page_in_area(&self, page: u64, mem_pages: u64) -> bool {
        let unused_stack = self
            .stack
            .is_some_and(|stack| (stack.first..stack.low).contains(&page));
        !unused_stack && self.page_mapped(page, mem_pages)
    }

    /// Whether `page` is in use: the image's pages, pages handed out, and pages grown by
    /// someone else; nothing at or past `mem_pages`.
    fn page_mapped(&self, page: u64, mem_pages: u64) -> bool {
        if page >= mem_pages {
            return false;
        }
        match self.base {
            Some(base) if page >= base => self
                .mapped
                .get((page - base) as usize)
                .copied()
                .unwrap_or(true),
            _ => true,
        }
    }

    /// Whether every page of `[addr, addr + len)` is in use.
    pub(crate) fn range_mapped(&self, addr: u64, len: u64, mem_size: u64) -> bool {
        let mem_pages = mem_size / PAGE_SIZE;
        (addr / PAGE_SIZE..(addr + len).div_ceil(PAGE_SIZE))
            .all(|page| self.page_mapped(page, mem_pages))
    }

    /// Whether `[first, end)` can be taken: each page is the allocator's and free, or at or past
    /// the end of memory.
    fn pages_free(&self, first: u64, end: u64, mem_pages: u64) -> bool {
        let movable = |page: u64| self.base.is_some_and(|base| page >= base) || page >= mem_pages;
        (first..end).all(|page| movable(page) && !self.page_mapped(page, mem_pages))
    }

    fn regions(&self) -> Vec<Region> {
        let mem_pages = self.seen.div_ceil(PAGE_SIZE);
        let image_end = self.base.unwrap_or(mem_pages).min(mem_pages);
        let mut regions = Vec::new();
        if image_end > 0 {
            regions.push(Region {
                start: 0,
                end: image_end * PAGE_SIZE,
                kind: RegionKind::Image,
            });
        }
        let heap = self
            .brk
            .map(|(start, end)| (start / PAGE_SIZE, end.div_ceil(PAGE_SIZE)));
        let kind = |page: u64| match heap {
            Some((start, end)) if (start..end).contains(&page) => RegionKind::Heap,
            _ => RegionKind::Anon,
        };
        for page in image_end..mem_pages {
            if !self.page_mapped(page, mem_pages) {
                continue;
            }
            let (start, end, kind) = (page * PAGE_SIZE, (page + 1) * PAGE_SIZE, kind(page));
            match regions.last_mut() {
                Some(last) if last.end == start && last.kind == kind => last.end = end,
                _ => regions.push(Region { start, end, kind }),
            }
        }
        regions
    }

    fn free_run(&self, pages: u64) -> Option<usize> {
        let pages = usize::try_from(pages).ok()?;
        let mut run = 0;
        for (index, &mapped) in self.mapped.iter().enumerate() {
            run = if mapped { 0 } else { run + 1 };
            if run == pages {
                return Some(index + 1 - pages);
            }
        }
        None
    }
}

/// Zeroes `[addr, addr + len)`, private pages of the address space, as a fresh anonymous mapping
/// must read. Where the memory can, the pages are dropped rather than written
/// ([`UserMemory::zero_pages`]), so reusing pages no one touched commits no host memory.
pub(crate) async fn zero(memory: &dyn UserMemory, addr: u64, len: u64) -> Result<(), Errno> {
    let at = u32::try_from(addr).map_err(|_| Errno::ENOMEM)?;
    memory.zero_pages(at, len).await.map_err(|_| Errno::ENOMEM)
}

/// Copies `len` bytes from `from` to `to`, which do not overlap.
async fn copy(memory: &dyn UserMemory, from: u64, to: u64, len: u64) -> Result<(), Errno> {
    let mut buf = vec![0; PAGE_SIZE as usize];
    let mut done = 0;
    while done < len {
        let n = (len - done).min(PAGE_SIZE) as usize;
        let fault = |_| Errno::EFAULT;
        memory
            .force_read((from + done) as u32, &mut buf[..n])
            .await
            .map_err(fault)?;
        memory
            .force_write((to + done) as u32, &buf[..n])
            .await
            .map_err(fault)?;
        done += n as u64;
    }
    Ok(())
}

/// Pages a mapping of `len` bytes needs; `EINVAL` for zero, as Linux.
pub(crate) fn pages(len: u32) -> Result<u64, Errno> {
    match u64::from(len).div_ceil(PAGE_SIZE) {
        0 => Err(Errno::EINVAL),
        pages => Ok(pages),
    }
}

/// The caller's main stack and stack pointer, for [`AddressSpace::expand_stack`].
async fn stack_seen(caller: &Caller<'_>) -> (Range<u32>, u32) {
    let memory = caller.memory();
    (memory.main_stack().await, memory.stack_pointer().await)
}

/// The pages `mlock` and `munlock` cover: from the one `start` is in through the one the last
/// byte is in.
fn lock_range(start: Uptr, len: Usize) -> (u64, u64) {
    let start = u64::from(start);
    (
        start / PAGE_SIZE,
        (start + u64::from(len)).div_ceil(PAGE_SIZE),
    )
}

fn check_aligned(addr: u32) -> Result<(), Errno> {
    match u64::from(addr).is_multiple_of(PAGE_SIZE) {
        true => Ok(()),
        false => Err(Errno::EINVAL),
    }
}

/// The arguments of an `mremap` that passed `check_mremap_params`, with the lengths in whole
/// pages.
struct Remap {
    addr: u64,
    old_size: u64,
    new_size: u64,
    new_addr: Uptr,
    may_move: bool,
    fixed: bool,
    dont_unmap: bool,
}

impl Remap {
    /// `check_mremap_params`, in its order.
    fn check(
        addr: Uptr,
        old_len: Ulong,
        new_len: Ulong,
        flags: MremapFlags,
        new_addr: Uptr,
    ) -> Result<Self, Errno> {
        let known =
            MremapFlags::MREMAP_MAYMOVE | MremapFlags::MREMAP_FIXED | MremapFlags::MREMAP_DONTUNMAP;
        if !(flags & !known).is_empty() {
            return Err(Errno::EINVAL);
        }
        check_aligned(addr)?;
        let remap = Self {
            addr: addr.into(),
            old_size: u64::from(old_len).div_ceil(PAGE_SIZE) * PAGE_SIZE,
            // A zero old length is the "duplicate" case; a zero new length is nonsense.
            new_size: pages(new_len)? * PAGE_SIZE,
            new_addr,
            may_move: flags.contains(MremapFlags::MREMAP_MAYMOVE),
            fixed: flags.contains(MremapFlags::MREMAP_FIXED),
            dont_unmap: flags.contains(MremapFlags::MREMAP_DONTUNMAP),
        };
        if !remap.implies_new_addr() {
            return Ok(remap);
        }
        let target = u64::from(new_addr);
        if target + remap.new_size > 1 << 32 {
            return Err(Errno::EINVAL);
        }
        check_aligned(new_addr)?;
        if !remap.may_move || (remap.dont_unmap && remap.old_size != remap.new_size) {
            return Err(Errno::EINVAL);
        }
        // `vrm_overlaps`: moving onto the source would copy what replacing the target zeroed.
        if remap.addr < target + remap.new_size && target < remap.addr + remap.old_size {
            return Err(Errno::EINVAL);
        }
        Ok(remap)
    }

    /// `vrm_implies_new_addr`: the mapping moves to `new_addr`, or with `MREMAP_DONTUNMAP` alone
    /// anywhere (the address is a hint, which is ignored).
    fn implies_new_addr(&self) -> bool {
        self.fixed || self.dont_unmap
    }
}

/// `MADV_*` values Linux's `madvise_behavior_valid` accepts.
fn advice_valid(advice: MadviseAdvice) -> bool {
    // `MADV_HWPOISON` and `MADV_SOFT_OFFLINE` (100, 101) poison or offline a page, which needs
    // `CONFIG_MEMORY_FAILURE`: without it Linux does not know them (`madvise_behavior_valid`).
    matches!(advice.0, 0..=4 | 8..=25 | 102..=103)
}

/// What a caller may lock: its soft `RLIMIT_MEMLOCK`, and whether `CAP_IPC_LOCK` lifts it.
struct LockLimit {
    bytes: u64,
    exempt: bool,
}

impl LockLimit {
    /// `can_do_mlock`: a limit of 0 forbids locking at all.
    fn can_lock(&self) -> bool {
        self.bytes != 0 || self.exempt
    }

    /// The limit in pages.
    fn pages(&self) -> u64 {
        self.bytes / PAGE_SIZE
    }

    /// `mlock_future_ok`: whether `bytes` more of a mapping that is `locked` fit beside the
    /// `locked_vm` pages already locked.
    fn future_ok(&self, locked: bool, locked_vm: u64, bytes: u64) -> bool {
        !locked || self.exempt || bytes / PAGE_SIZE + locked_vm <= self.pages()
    }
}

impl Kernel {
    fn lock_limit(&self, caller: &Caller<'_>) -> LockLimit {
        let process = self.process(caller.task());
        let bytes = process
            .resources
            .lock_or_recover()
            .soft(wasixv2_abi::RlimitResource::RLIMIT_MEMLOCK);
        LockLimit {
            bytes,
            // `can_do_mlock`: `capable(CAP_IPC_LOCK)`.
            exempt: process.cred().capable_init(Cap::IpcLock),
        }
    }

    /// `mlock_future_ok` for a new mapping of `len` bytes, locked if `MAP_LOCKED` asks or
    /// `mlockall(MCL_FUTURE)` did: `EAGAIN` if it does not fit. Returns whether it is locked.
    fn check_lock_new(
        &self,
        caller: &Caller<'_>,
        map_locked: bool,
        len: u64,
    ) -> Result<bool, Errno> {
        let limit = self.lock_limit(caller);
        let mm = self.mm(caller.task());
        let space = mm.space();
        let locked = map_locked || space.lock_future;
        match limit.future_ok(locked, space.locked_vm(), len) {
            true => Ok(locked),
            false => Err(Errno::EAGAIN),
        }
    }

    /// `resize_is_valid`'s `mlock_future_ok`: the area at `addr` grows by `delta` bytes, which
    /// must fit if it is locked (`EAGAIN`). Returns whether it is.
    fn check_lock_growth(&self, caller: &Caller<'_>, addr: u64, delta: u64) -> Result<bool, Errno> {
        let limit = self.lock_limit(caller);
        let mm = self.mm(caller.task());
        let space = mm.space();
        let locked = space.locked.get(addr / PAGE_SIZE);
        match limit.future_ok(locked, space.locked_vm(), delta) {
            true => Ok(locked),
            false => Err(Errno::EAGAIN),
        }
    }

    pub(crate) async fn mem_size(&self, caller: &Caller<'_>) -> u64 {
        let size = caller.memory().size().await;
        self.mm(caller.task()).space().seen = size;
        size
    }

    /// Fails with `ENOMEM` unless all of `[addr, addr + len)` is mapped.
    async fn check_mapped(&self, caller: &Caller<'_>, addr: u64, len: u64) -> Result<(), Errno> {
        let size = self.mem_size(caller).await;
        let mm = self.mm(caller.task());
        match mm.space().range_mapped(addr, len, size) {
            true => Ok(()),
            false => Err(Errno::ENOMEM),
        }
    }

    /// Takes the pages `[first, end)` for a mapping or the break, growing memory for those past
    /// its end, and zeroes those that were mapped before. They must be free.
    async fn take_pages(&self, caller: &Caller<'_>, first: u64, end: u64) -> Result<(), Errno> {
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let mm = self.mm(caller.task());
        if end > mem_pages {
            let grow = u32::try_from(end - mem_pages).map_err(|_| Errno::ENOMEM)?;
            let previous = caller.memory().grow(grow).await.ok_or(Errno::ENOMEM)?;
            let mut space = mm.space();
            space.grown(previous, grow);
            if previous != mem_pages * PAGE_SIZE {
                // Someone else grew memory in between; give up rather than guess.
                space.set_pages(
                    previous / PAGE_SIZE,
                    previous / PAGE_SIZE + grow as u64,
                    false,
                );
                return Err(Errno::ENOMEM);
            }
        }
        mm.space().set_pages(first, end, true);
        let reused_end = end.min(mem_pages);
        if first < reused_end {
            zero(
                caller.memory(),
                first * PAGE_SIZE,
                (reused_end - first) * PAGE_SIZE,
            )
            .await?;
        }
        Ok(())
    }

    /// The break starts at the end of memory as it is on the first call.
    pub(crate) async fn sys_brk(&self, caller: &Caller<'_>, brk: Uptr) -> SysResult<i32> {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let size = self.mem_size(caller).await;
        let (start, current) = *mm.space().brk.get_or_insert((size, size));
        let new = u64::from(brk);
        if new < start {
            return Ok(current as i32);
        }
        let (old_end, new_end) = (current.div_ceil(PAGE_SIZE), new.div_ceil(PAGE_SIZE));
        if new_end > old_end {
            // `check_brk_limits`.
            let free = mm.space().pages_free(old_end, new_end, size / PAGE_SIZE);
            let len = (new_end - old_end) * PAGE_SIZE;
            let Ok(locked) = self.check_lock_new(caller, false, len) else {
                return Ok(current as i32);
            };
            if !free || self.take_pages(caller, old_end, new_end).await.is_err() {
                return Ok(current as i32);
            }
            mm.space().locked.set(old_end, new_end, locked);
        } else {
            let (from, to) = (new_end * PAGE_SIZE, old_end * PAGE_SIZE);
            mm.space().unmap(from, to - from);
        }
        mm.space().brk = Some((start, new));
        Ok(new as i32)
    }

    /// `do_mmap`. File mappings copy the file in; see the module docs.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sys_mmap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        len: Ulong,
        prot: ProtFlags,
        flags: MapFlags,
        fd: Fd,
        offset: Loff,
    ) -> SysResult<Uptr> {
        if offset % PAGE_SIZE as i64 != 0 {
            return Err(Errno::EINVAL.into());
        }
        let anonymous = flags.contains(MapFlags::MAP_ANONYMOUS);
        let file = match anonymous {
            true => None,
            false => Some(self.mmap_file(caller, fd)?),
        };
        if prot.contains(ProtFlags::PROT_EXEC) {
            return Err(Errno::EACCES.into());
        }
        let pages = pages(len)?;
        let map_locked = flags.contains(MapFlags::MAP_LOCKED);
        if map_locked && !self.lock_limit(caller).can_lock() {
            return Err(Errno::EPERM.into());
        }
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let locked = self.check_lock_new(caller, map_locked, pages * PAGE_SIZE)?;
        let kind = flags & MapFlags::MAP_TYPE;
        let source = match &file {
            None => match kind {
                MapFlags::MAP_SHARED => self.shared_anonymous(pages)?,
                MapFlags::MAP_PRIVATE => None,
                _ => return Err(Errno::EINVAL.into()),
            },
            Some(file) => Some(check_file_mapping(file, offset, pages, prot, flags)?),
        };
        let start = if flags.intersects(MapFlags::MAP_FIXED | MapFlags::MAP_FIXED_NOREPLACE) {
            let noreplace = !flags.contains(MapFlags::MAP_FIXED);
            self.map_fixed(caller, addr, pages, noreplace).await?
        } else {
            self.map_anywhere(caller, pages).await? as Uptr
        };
        let first = u64::from(start) / PAGE_SIZE;
        {
            let mut space = mm.space();
            space.locked.set(first, first + pages, locked);
            space.set_prot(first, first + pages, prot);
        }
        let Some(source) = source else {
            return Ok(start);
        };
        let mut mapping = Mapping {
            start: start.into(),
            len: pages * PAGE_SIZE,
            source,
        };
        let mut filled = mapping.fill(caller.memory()).await;
        if filled == Err(Errno::EOPNOTSUPP)
            && let Some(file) = file
            && let Source::Mapped(mapped) = &mut mapping.source
        {
            // Memory that cannot show a file's pages gets a copy of them.
            let (offset, writer) = (mapped.offset, mapped.writer.take());
            let shared = Shared::new(Object::File(file), offset, mapped.writable, writer);
            mapping.source = Source::Shared(shared);
            filled = mapping.fill(caller.memory()).await;
        }
        match filled {
            Ok(()) => mm.insert(mapping),
            // Memory that cannot show the object's pages keeps the zeroed pages it has, shared
            // only within the address space.
            Err(Errno::EOPNOTSUPP) if mapping.mapped().is_some() => {}
            Err(errno) => {
                mm.space().unmap(start.into(), pages * PAGE_SIZE);
                return Err(errno.into());
            }
        }
        Ok(start)
    }

    /// The object of a new `MAP_SHARED | MAP_ANONYMOUS` mapping of `pages` pages
    /// (`shmem_zero_setup`), or `None` from a host without shared memory objects: the mapping
    /// is then ordinary memory of the address space, which no other process sees.
    fn shared_anonymous(&self, pages: u64) -> Result<Option<Source>, Errno> {
        match self.host.shared_pages(pages * PAGE_SIZE) {
            Ok(pages) => Ok(Some(Source::Mapped(Mapped {
                pages,
                offset: 0,
                segment: None,
                writable: true,
                writer: None,
            }))),
            Err(err) if err.kind() == io::ErrorKind::Unsupported => Ok(None),
            Err(_) => Err(Errno::ENOMEM),
        }
    }

    /// The open file `mmap` maps: `EBADF` unless `fd` is open, and not just as a location.
    fn mmap_file(&self, caller: &Caller<'_>, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let file = self.file(caller.task(), fd)?;
        if file.flags().contains(OpenFlags::O_PATH) {
            return Err(Errno::EBADF);
        }
        Ok(file)
    }

    /// Maps `pages` zeroed pages wherever they fit. The caller holds `Mm::op`.
    async fn map_anywhere(&self, caller: &Caller<'_>, pages: u64) -> Result<u64, Errno> {
        let mm = self.mm(caller.task());
        let placement = mm.space().place(pages)?;
        let addr = match placement {
            Placement::Reuse(addr) => {
                zero(caller.memory(), addr, pages * PAGE_SIZE).await?;
                addr
            }
            Placement::Grow(pages) => {
                let previous = caller.memory().grow(pages).await.ok_or(Errno::ENOMEM)?;
                mm.space().grown(previous, pages);
                previous
            }
        };
        Ok(addr)
    }

    /// `MAP_FIXED` works over pages the allocator owns and past the end of memory; the image's
    /// own pages cannot be replaced, and nothing goes below [`MMAP_MIN_ADDR`] (`EPERM`, as
    /// `security_mmap_addr` refuses it once `get_unmapped_area` checked the range). Mappings it
    /// replaces are unmapped first. The caller holds `Mm::op`.
    async fn map_fixed(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        pages: u64,
        noreplace: bool,
    ) -> SysResult<Uptr> {
        check_aligned(addr)?;
        let first = u64::from(addr) / PAGE_SIZE;
        let end = first + pages;
        if end * PAGE_SIZE > 1 << 32 {
            return Err(Errno::ENOMEM.into());
        }
        if u64::from(addr) < MMAP_MIN_ADDR {
            return Err(Errno::EPERM.into());
        }
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let mm = self.mm(caller.task());
        let replace = {
            let space = mm.space();
            if space.pages_free(first, end, mem_pages) {
                // Free pages can be taken as they are.
                false
            } else if noreplace {
                return Err(Errno::EEXIST.into());
            } else {
                let owned = |page: u64| space.base.is_some_and(|base| page >= base);
                if !(first..end).all(|page| owned(page) || page >= mem_pages) {
                    return Err(Errno::EINVAL.into());
                }
                true
            }
        };
        if replace {
            self.unmap_range(caller, first * PAGE_SIZE, end * PAGE_SIZE)
                .await;
        }
        // Pages between the end of memory and the mapping stay free.
        self.take_pages(caller, first, end).await?;
        if first > mem_pages {
            mm.space().set_pages(mem_pages, first, false);
        }
        Ok(addr)
    }

    /// `do_vmi_munmap`: writes back the shared mappings in `[start, end)`, takes mapped objects'
    /// pages out, drops every mapping there and frees the pages. The caller holds `Mm::op`.
    async fn unmap_range(&self, caller: &Caller<'_>, start: u64, end: u64) {
        let mm = self.mm(caller.task());
        for mut mapping in mm.take_range(start, end) {
            // munmap cannot fail once the range is valid; a file that refuses the data loses it.
            let _ = mapping.release(caller.memory()).await;
        }
        mm.space().unmap(start, end - start);
    }

    pub(crate) async fn sys_munmap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        len: Usize,
    ) -> SysResult<i32> {
        check_aligned(addr)?;
        if len == 0 {
            return Err(Errno::EINVAL.into());
        }
        let start = u64::from(addr);
        let end = (start + u64::from(len)).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        self.unmap_range(caller, start, end).await;
        Ok(0)
    }

    /// `do_mremap` (`mm/mremap.c`).
    pub(crate) async fn sys_mremap(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        old_len: Ulong,
        new_len: Ulong,
        flags: MremapFlags,
        new_addr: Uptr,
    ) -> SysResult<i32> {
        let remap = Remap::check(addr, old_len, new_len, flags, new_addr)?;
        let (addr64, old_size, new_size) = (remap.addr, remap.old_size, remap.new_size);
        let new_pages = new_size / PAGE_SIZE;
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        if old_size == 0 {
            return self.mremap_duplicate(caller, &remap).await;
        }
        self.check_mapped(caller, addr64, old_size)
            .await
            .map_err(|_| Errno::EFAULT)?;
        if new_size > old_size {
            self.check_lock_growth(caller, addr64, new_size - old_size)?;
        }
        if remap.implies_new_addr() {
            let target = match remap.fixed {
                true => u64::from(self.map_fixed(caller, new_addr, new_pages, false).await?),
                false => self.map_anywhere(caller, new_pages).await?,
            };
            // `MREMAP_DONTUNMAP` leaves the source mapped, as it was before its pages moved.
            let stay = match remap.dont_unmap {
                true => mm.space().mappings.aliases(addr64, addr64 + old_size),
                false => Vec::new(),
            };
            copy(caller.memory(), addr64, target, old_size.min(new_size)).await?;
            mm.space().remap_flags(addr64, old_size, target, new_size);
            self.move_mappings(caller, addr64, old_size, target, new_size)
                .await?;
            match remap.dont_unmap {
                true => {
                    // `dontunmap_complete`: the source is never locked after the move.
                    let first = addr64 / PAGE_SIZE;
                    mm.space()
                        .locked
                        .set(first, first + old_size / PAGE_SIZE, false);
                    self.refault(caller, addr64, old_size, stay).await?
                }
                false => mm.space().unmap(addr64, old_size),
            }
            return Ok(target as i32);
        }
        let (first, old_pages) = (addr64 / PAGE_SIZE, old_size / PAGE_SIZE);
        if new_pages <= old_pages {
            self.unmap_range(caller, addr64 + new_size, addr64 + old_size)
                .await;
            return Ok(addr as i32);
        }
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let grow_in_place = mm
            .space()
            .pages_free(first + old_pages, first + new_pages, mem_pages);
        if grow_in_place
            && self
                .take_pages(caller, first + old_pages, first + new_pages)
                .await
                .is_ok()
        {
            mm.space().remap_flags(addr64, old_size, addr64, new_size);
            self.move_mappings(caller, addr64, old_size, addr64, new_size)
                .await?;
            return Ok(addr as i32);
        }
        if !remap.may_move {
            return Err(Errno::ENOMEM.into());
        }
        let target = self.map_anywhere(caller, new_pages).await?;
        copy(caller.memory(), addr64, target, old_size).await?;
        mm.space().remap_flags(addr64, old_size, target, new_size);
        self.move_mappings(caller, addr64, old_size, target, new_size)
            .await?;
        mm.space().unmap(addr64, old_size);
        Ok(target as i32)
    }

    /// `mremap` with an old length of 0: a second mapping of the shared mapping at `addr`
    /// (Linux's "duplicate shm area"). Anonymous shared memory the host could not give an object
    /// is private here (see `sys_mmap`), so like a private mapping it cannot be duplicated. The
    /// caller holds `Mm::op`.
    async fn mremap_duplicate(&self, caller: &Caller<'_>, remap: &Remap) -> SysResult<i32> {
        let mm = self.mm(caller.task());
        self.check_mapped(caller, remap.addr, 1)
            .await
            .map_err(|_| Errno::EFAULT)?;
        let alias = |target: u64| {
            let space = mm.space();
            let mapping = space.mappings.containing(remap.addr)?;
            mapping
                .is_shared()
                .then(|| mapping.alias(remap.addr - mapping.start, target, remap.new_size))
        };
        if alias(0).is_none() {
            return Err(Errno::EINVAL.into());
        }
        let locked = self.check_lock_growth(caller, remap.addr, remap.new_size)?;
        // Growing a mapping of nothing in place never fits: `addr` itself is mapped.
        if !remap.may_move {
            return Err(Errno::ENOMEM.into());
        }
        let pages = remap.new_size / PAGE_SIZE;
        let target = match remap.fixed {
            true => u64::from(self.map_fixed(caller, remap.new_addr, pages, false).await?),
            false => self.map_anywhere(caller, pages).await?,
        };
        // A fixed target may have replaced the source (`vma_lookup` after `do_munmap`).
        let Some(mut alias) = alias(target) else {
            mm.space().unmap(target, remap.new_size);
            return Err(Errno::EFAULT.into());
        };
        if let Err(errno) = alias.fill(caller.memory()).await {
            mm.space().unmap(target, remap.new_size);
            return Err(errno.into());
        }
        mm.insert(alias);
        let first = target / PAGE_SIZE;
        mm.space()
            .locked
            .set(first, first + remap.new_size / PAGE_SIZE, locked);
        Ok(target as i32)
    }

    /// The source of a `MREMAP_DONTUNMAP` move after its pages left: anonymous memory reads as
    /// zeroes, and the mappings that were there (`stay`) read as their source again, as the
    /// page faults that follow would fill them. The caller holds `Mm::op`.
    async fn refault(
        &self,
        caller: &Caller<'_>,
        addr: u64,
        len: u64,
        stay: Vec<Mapping>,
    ) -> Result<(), Errno> {
        zero(caller.memory(), addr, len).await?;
        let mut filled = Vec::with_capacity(stay.len());
        let mut result = Ok(());
        for mut mapping in stay {
            result = result.and(mapping.fill(caller.memory()).await);
            filled.push(mapping);
        }
        self.mm(caller.task()).restore(filled);
        result
    }

    /// The mappings of `[from, from + old_len)` follow `mremap` to `to` with `new_len` bytes,
    /// whose memory already holds the old contents: parts past the new length are written back
    /// and dropped, mapped objects' pages move along, and a mapping that ended at the old end
    /// grows with more of its source. The caller holds `Mm::op`.
    async fn move_mappings(
        &self,
        caller: &Caller<'_>,
        from: u64,
        old_len: u64,
        to: u64,
        new_len: u64,
    ) -> Result<(), Errno> {
        let mm = self.mm(caller.task());
        let mut moved = mm.take_range(from, from + old_len);
        let keep = old_len.min(new_len);
        let mut kept = Vec::new();
        let mut result = Ok(());
        for mut mapping in moved.drain(..) {
            if mapping.start >= from + keep {
                let _ = mapping.release(caller.memory()).await;
                continue;
            }
            if mapping.end() > from + keep {
                let mut tail = mapping.split_off(from + keep);
                let _ = tail.release(caller.memory()).await;
            }
            let old = mapping.start;
            mapping.start = mapping.start - from + to;
            if mapping.start != old {
                result = result.and(mapping.relocate(caller.memory(), old).await);
            }
            kept.push(mapping);
        }
        if new_len > old_len
            && let Some(last) = kept.last_mut()
            && last.end() == to + old_len
        {
            result = result.and(last.grow(caller.memory(), new_len - old_len).await);
        }
        mm.restore(kept);
        result
    }

    pub(crate) async fn sys_mprotect(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        prot: ProtFlags,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        let known = ProtFlags::PROT_READ
            | ProtFlags::PROT_WRITE
            | ProtFlags::PROT_EXEC
            | ProtFlags::PROT_SEM
            | ProtFlags::PROT_GROWSDOWN
            | ProtFlags::PROT_GROWSUP;
        let grows = ProtFlags::PROT_GROWSDOWN | ProtFlags::PROT_GROWSUP;
        if !(prot & !known).is_empty() || prot.contains(grows) {
            return Err(Errno::EINVAL.into());
        }
        if len == 0 {
            return Ok(0);
        }
        self.check_mapped(caller, start.into(), len.into()).await?;
        if prot.contains(ProtFlags::PROT_EXEC) {
            return Err(Errno::EACCES.into());
        }
        // Write access a shared mapping could never have (`VM_MAYWRITE`).
        let (start, end) = (u64::from(start), u64::from(start) + u64::from(len));
        let mm = self.mm(caller.task());
        let mut space = mm.space();
        if prot.contains(ProtFlags::PROT_WRITE) && space.mappings.read_only_shared_in(start, end) {
            return Err(Errno::EACCES.into());
        }
        space.set_prot(start / PAGE_SIZE, end.div_ceil(PAGE_SIZE), prot);
        Ok(0)
    }

    /// `MADV_DONTNEED` drops the pages: anonymous memory reads as zeroes again, a private file
    /// mapping as the file, and a shared mapping keeps its contents, which are synchronised
    /// unless the memory shows the object's pages itself. `MADV_WIPEONFORK` and
    /// `MADV_KEEPONFORK` set and clear `VM_WIPEONFORK`. Every other advice changes nothing.
    pub(crate) async fn sys_madvise(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        advice: MadviseAdvice,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        if !advice_valid(advice) {
            return Err(Errno::EINVAL.into());
        }
        if len == 0 {
            return Ok(0);
        }
        let len = u64::from(len).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        self.check_mapped(caller, start.into(), len).await?;
        match advice {
            MadviseAdvice::MADV_DONTNEED => self.drop_pages(caller, start.into(), len).await?,
            MadviseAdvice::MADV_REMOVE => return Err(Errno::EINVAL.into()),
            MadviseAdvice::MADV_WIPEONFORK => {
                self.madvise_wipe_on_fork(caller, start.into(), len, true)
                    .await?
            }
            MadviseAdvice::MADV_KEEPONFORK => {
                self.madvise_wipe_on_fork(caller, start.into(), len, false)
                    .await?
            }
            _ => {}
        }
        Ok(0)
    }

    /// Every mapped page is resident.
    pub(crate) async fn sys_mincore(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        vec: Buf,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        self.check_mapped(caller, start.into(), len.into()).await?;
        let pages = u64::from(len).div_ceil(PAGE_SIZE) as usize;
        caller.memory().write(vec.addr(), &vec![1; pages]).await?;
        Ok(0)
    }

    pub(crate) async fn sys_msync(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MsyncFlags,
    ) -> SysResult<i32> {
        check_aligned(start)?;
        let known = MsyncFlags::MS_ASYNC | MsyncFlags::MS_INVALIDATE | MsyncFlags::MS_SYNC;
        if !(flags & !known).is_empty()
            || flags.contains(MsyncFlags::MS_ASYNC | MsyncFlags::MS_SYNC)
        {
            return Err(Errno::EINVAL.into());
        }
        let len = u64::from(len).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        let start = u64::from(start);
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        self.check_mapped(caller, start, len).await?;
        // `MS_INVALIDATE` cannot drop locked pages (`VM_LOCKED`). Linux finds that at the first
        // locked area, after flushing the areas before it; here nothing is synchronised then.
        let (first, end) = (start / PAGE_SIZE, (start + len) / PAGE_SIZE);
        if flags.contains(MsyncFlags::MS_INVALIDATE) && mm.space().locked.count(first, end) > 0 {
            return Err(Errno::EBUSY.into());
        }
        let mut mappings = mm.take_range(start, start + len);
        let mut result = Ok(());
        for mapping in &mut mappings {
            let Some(shared) = mapping.shared() else {
                continue;
            };
            let file = shared.object.file().cloned();
            let synced = mapping.sync(caller.memory()).await;
            let synced = match (synced, file) {
                (Ok(()), Some(file)) if flags.contains(MsyncFlags::MS_SYNC) => {
                    file.inode()?.sync(true).await
                }
                (synced, _) => synced,
            };
            result = result.and(synced);
        }
        mm.restore(mappings);
        result?;
        Ok(0)
    }

    /// `MADV_DONTNEED` over `[start, start + len)`.
    async fn drop_pages(&self, caller: &Caller<'_>, start: u64, len: u64) -> Result<(), Errno> {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let mut mappings = mm.take_range(start, start + len);
        let mut at = start;
        let mut result = Ok(());
        for mapping in &mut mappings {
            if mapping.start > at {
                result = result.and(zero(caller.memory(), at, mapping.start - at).await);
            }
            let refreshed = match &mapping.source {
                Source::Shared(_) => mapping.sync(caller.memory()).await,
                Source::Mapped(_) => Ok(()),
                Source::Private { .. } => mapping.fill(caller.memory()).await,
            };
            result = result.and(refreshed);
            at = mapping.end();
        }
        if at < start + len {
            result = result.and(zero(caller.memory(), at, start + len - at).await);
        }
        mm.restore(mappings);
        result
    }

    /// `MADV_WIPEONFORK` (`wipe`) or `MADV_KEEPONFORK` over `[start, start + len)`. Only
    /// private anonymous memory can be wiped: `madvise_update_vma` goes through the areas in
    /// order, so the pages before the first file or shared mapping in the range take the flag,
    /// and that mapping fails with `EINVAL`. Anonymous shared memory the host could not give an
    /// object is private memory here (see `sys_mmap`), and takes it too.
    async fn madvise_wipe_on_fork(
        &self,
        caller: &Caller<'_>,
        start: u64,
        len: u64,
        wipe: bool,
    ) -> Result<(), Errno> {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let mut space = mm.space();
        let end = start + len;
        let refused = match wipe {
            true => space
                .mappings
                .first_in(start, end)
                .map(|m| m.start.max(start)),
            false => None,
        };
        let marked_end = refused.unwrap_or(end);
        space
            .wipe_on_fork
            .set(start / PAGE_SIZE, marked_end / PAGE_SIZE, wipe);
        match refused {
            Some(_) => Err(Errno::EINVAL),
            None => Ok(()),
        }
    }

    /// The ranges of `task`'s memory a fork child gets zeroed (`VM_WIPEONFORK`), lowest first:
    /// the execution layer leaves them out of the child's copy. None for a task that is gone.
    pub(crate) fn wipe_on_fork(&self, task: TaskId) -> Vec<Range<u64>> {
        let mm = {
            let state = self.state.read_or_recover();
            match state.tasks.get(&task) {
                Some(task) => task.mm.clone(),
                None => return Vec::new(),
            }
        };
        let space = mm.space();
        space.wipe_on_fork.ranges()
    }

    /// `do_mlock` for `mlock` and `mlock2`. Memory is never paged out, so locking only
    /// accounts the pages against `RLIMIT_MEMLOCK` (`MLOCK_ONFAULT` changes nothing).
    pub(crate) async fn sys_mlock(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
        flags: MlockFlags,
    ) -> SysResult<i32> {
        if !(flags & !MlockFlags::MLOCK_ONFAULT).is_empty() {
            return Err(Errno::EINVAL.into());
        }
        let limit = self.lock_limit(caller);
        if !limit.can_lock() {
            return Err(Errno::EPERM.into());
        }
        let (first, end) = lock_range(start, len);
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let (stack, sp) = stack_seen(caller).await;
        let mut space = mm.space();
        space.expand_stack(stack, sp);
        let mut locked = end - first + space.locked_vm();
        if locked > limit.pages() && !limit.exempt {
            // Pages already locked in the range are not locked again.
            locked -= space.locked.count(first, end);
        }
        if locked > limit.pages() && !limit.exempt {
            return Err(Errno::ENOMEM.into());
        }
        space.lock_pages(first, end, true, mem_pages)?;
        Ok(0)
    }

    pub(crate) async fn sys_munlock(
        &self,
        caller: &Caller<'_>,
        start: Uptr,
        len: Usize,
    ) -> SysResult<i32> {
        let (first, end) = lock_range(start, len);
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let (stack, sp) = stack_seen(caller).await;
        let mut space = mm.space();
        space.expand_stack(stack, sp);
        space.lock_pages(first, end, false, mem_pages)?;
        Ok(0)
    }

    /// `mlockall`: `MCL_CURRENT` locks every page in use, which must all fit under
    /// `RLIMIT_MEMLOCK`; `MCL_FUTURE` locks every mapping made from then on.
    pub(crate) async fn sys_mlockall(
        &self,
        caller: &Caller<'_>,
        flags: MclFlags,
    ) -> SysResult<i32> {
        let known = MclFlags::MCL_CURRENT | MclFlags::MCL_FUTURE | MclFlags::MCL_ONFAULT;
        let lock = MclFlags::MCL_CURRENT | MclFlags::MCL_FUTURE;
        if !(flags & !known).is_empty() || !flags.intersects(lock) {
            return Err(Errno::EINVAL.into());
        }
        let limit = self.lock_limit(caller);
        if !limit.can_lock() {
            return Err(Errno::EPERM.into());
        }
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let mem_pages = self.mem_size(caller).await / PAGE_SIZE;
        let (stack, sp) = stack_seen(caller).await;
        let mut space = mm.space();
        space.expand_stack(stack, sp);
        let current = flags.contains(MclFlags::MCL_CURRENT);
        if current && space.total_vm(mem_pages) > limit.pages() && !limit.exempt {
            return Err(Errno::ENOMEM.into());
        }
        space.lock_all(flags);
        Ok(0)
    }

    /// `munlockall`.
    pub(crate) async fn sys_munlockall(&self, caller: &Caller<'_>) -> SysResult<i32> {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        mm.space().lock_all(MclFlags::empty());
        Ok(0)
    }

    /// Makes the caller's shared mappings of `file`'s inode agree with it. Data syscalls on a
    /// file call it before they read or write it and after they wrote, so the process sees one
    /// copy of the file whichever way it looks.
    pub(crate) async fn sync_file_mappings(&self, caller: &Caller<'_>, file: &OpenFile) {
        let mm = self.mm(caller.task());
        if !mm.shared.load(Ordering::Acquire) {
            return;
        }
        let Ok(inode) = file.inode() else {
            return;
        };
        let key = inode.lock_key();
        let of_file = |mapping: &Mapping| {
            let file = mapping.shared().and_then(|shared| shared.object.file());
            file.and_then(|file| file.inode().ok())
                .is_some_and(|inode| inode.lock_key() == key)
        };
        self.sync_where(caller, of_file).await;
    }

    /// Synchronises every attachment of a SysV segment in the caller's address space; see
    /// `ipc::shm` for when.
    pub(crate) async fn sync_segments(&self, caller: &Caller<'_>) {
        let mm = self.mm(caller.task());
        if !mm.shared.load(Ordering::Acquire) {
            return;
        }
        let is_segment = |mapping: &Mapping| {
            mapping
                .shared()
                .is_some_and(|shared| matches!(shared.object, Object::Segment(_)))
        };
        self.sync_where(caller, is_segment).await;
    }

    async fn sync_where(&self, caller: &Caller<'_>, pick: impl FnMut(&Mapping) -> bool) {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let mut mappings = mm.space().mappings.take_where(pick);
        for mapping in &mut mappings {
            // Nothing to report to: the syscall that got here is about something else.
            let _ = mapping.sync(caller.memory()).await;
        }
        mm.restore(mappings);
    }

    /// The ranges of `task`'s memory that show shared objects' pages, lowest first: what a fork
    /// child's memory must show again over its copy (`dup_mmap` keeps a shared mapping on its
    /// object, `mm/mmap.c`). None for a task that is gone.
    pub(crate) fn shared_mappings(&self, task: TaskId) -> Vec<SharedMapping> {
        let mm = {
            let state = self.state.read_or_recover();
            match state.tasks.get(&task) {
                Some(task) => task.mm.clone(),
                None => return Vec::new(),
            }
        };
        let space = mm.space();
        space
            .mappings
            .mapped()
            .map(|(mapping, mapped)| SharedMapping {
                addr: mapping.start as u32,
                len: mapping.len,
                pages: mapped.pages.clone(),
                offset: mapped.offset,
                writable: mapped.writable,
            })
            .collect()
    }

    /// Whether a live process other than `task`'s runs on `task`'s address space: a vfork
    /// parent or child (`mm_users` above what the group holds).
    pub(crate) fn mm_shared_elsewhere(&self, task: TaskId) -> bool {
        let state = self.state.read_or_recover();
        let Some(me) = state.tasks.get(&task) else {
            return false;
        };
        state.tasks.values().any(|other| {
            !Arc::ptr_eq(&other.process, &me.process)
                && Arc::ptr_eq(&other.mm, &me.mm)
                && !other.process.group.exiting()
        })
    }

    /// `exit_mmap` while the memory is still there: writes back every shared mapping and drops
    /// all of them, for `exit`, `exit_group` and `execve`. An address space a vfork relative
    /// still runs on stays as it is (`mmput` of a user that is not the last).
    pub(crate) async fn mm_release(&self, caller: &Caller<'_>) {
        if self.mm_shared_elsewhere(caller.task()) {
            return;
        }
        self.exit_mmap(caller).await;
    }

    /// `exit_mmap` while the memory is still there, by its last user: writes back every shared
    /// mapping and drops all of them.
    pub(crate) async fn exit_mmap(&self, caller: &Caller<'_>) {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let mappings = mm.space().mappings.clear();
        for mut mapping in mappings {
            let _ = mapping.write_back(caller.memory()).await;
        }
        mm.shared.store(false, Ordering::Release);
    }

    /// Maps a SysV segment attachment of `len` bytes at `addr`, or anywhere for 0, replacing
    /// what is there only with `remap`. Returns the address. An attachment the memory cannot
    /// show the segment's pages in becomes a copy.
    pub(crate) async fn map_segment(
        &self,
        caller: &Caller<'_>,
        addr: Uptr,
        len: u64,
        remap: bool,
        source: Source,
    ) -> Result<Uptr, Errno> {
        let pages = len.div_ceil(PAGE_SIZE);
        // `do_shmat`: `PROT_READ`, and `PROT_WRITE` unless `SHM_RDONLY`.
        let prot = match &source {
            Source::Mapped(Mapped {
                writable: false, ..
            }) => ProtFlags::PROT_READ,
            Source::Shared(shared) if !shared.writable => ProtFlags::PROT_READ,
            _ => ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
        };
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let locked = self.check_lock_new(caller, false, pages * PAGE_SIZE)?;
        let start = match addr {
            0 => self.map_anywhere(caller, pages).await? as Uptr,
            addr => {
                let (first, end) = (
                    u64::from(addr) / PAGE_SIZE,
                    u64::from(addr) / PAGE_SIZE + pages,
                );
                let size = self.mem_size(caller).await;
                let busy = (first..end).any(|page| mm.space().page_mapped(page, size / PAGE_SIZE));
                if busy && !remap {
                    return Err(Errno::EINVAL);
                }
                self.map_fixed(caller, addr, pages, false)
                    .await
                    .map_err(|_| Errno::EINVAL)?
            }
        };
        let mut mapping = Mapping {
            start: start.into(),
            len: pages * PAGE_SIZE,
            source,
        };
        let mut filled = mapping.fill(caller.memory()).await;
        if filled == Err(Errno::EOPNOTSUPP)
            && let Source::Mapped(mapped) = &mapping.source
            && let Some(attachment) = &mapped.segment
        {
            let object = Object::Segment(attachment.clone());
            let shared = Shared::new(object, mapped.offset, mapped.writable, None);
            mapping.source = Source::Shared(shared);
            filled = mapping.fill(caller.memory()).await;
        }
        if let Err(errno) = filled {
            mm.space().unmap(start.into(), pages * PAGE_SIZE);
            return Err(errno);
        }
        mm.insert(mapping);
        let first = u64::from(start) / PAGE_SIZE;
        let mut space = mm.space();
        space.locked.set(first, first + pages, locked);
        space.set_prot(first, first + pages, prot);
        Ok(start)
    }

    /// `shmdt`: unmaps the attachment that starts at `addr`, every part of it. `EINVAL` if no
    /// attachment starts there.
    pub(crate) async fn unmap_segment(&self, caller: &Caller<'_>, addr: Uptr) -> Result<(), Errno> {
        let mm = self.mm(caller.task());
        let _op = mm.op.lock().await;
        let attachment = {
            let space = mm.space();
            let mapping = space.mappings.starting_at(addr.into());
            match mapping.and_then(Mapping::attachment) {
                Some(attachment) => attachment.clone(),
                None => return Err(Errno::EINVAL),
            }
        };
        let same = |mapping: &Mapping| {
            mapping
                .attachment()
                .is_some_and(|other| Arc::ptr_eq(other, &attachment))
        };
        let parts = mm.space().mappings.take_where(same);
        for mut part in parts {
            let _ = part.release(caller.memory()).await;
            mm.space().unmap(part.start, part.len);
        }
        let any = mm.space().mappings.any_shared();
        mm.shared.store(any, Ordering::Release);
        Ok(())
    }
}

/// The file checks of `do_mmap`, in its order, and the source of the mapping: the file's own
/// pages for a shared mapping of a file kept in a shared memory object (read-only where the
/// mapping may never be written), a copy otherwise.
fn check_file_mapping(
    file: &Arc<OpenFile>,
    offset: Loff,
    pages: u64,
    prot: ProtFlags,
    flags: MapFlags,
) -> Result<Source, Errno> {
    // `file_mmap_ok`: the range must fit below the largest file offset.
    let offset = u64::try_from(offset).map_err(|_| Errno::EOVERFLOW)?;
    if offset
        .checked_add(pages * PAGE_SIZE)
        .is_none_or(|end| end > i64::MAX as u64)
    {
        return Err(Errno::EOVERFLOW);
    }
    let writable = file.check_writable().is_ok();
    let shared = match flags & MapFlags::MAP_TYPE {
        MapFlags::MAP_SHARED | MapFlags::MAP_SHARED_VALIDATE => {
            let validate = flags & MapFlags::MAP_TYPE == MapFlags::MAP_SHARED_VALIDATE;
            if validate && !(flags & !LEGACY_MAP_MASK).is_empty() {
                return Err(Errno::EOPNOTSUPP);
            }
            if prot.contains(ProtFlags::PROT_WRITE) && !writable {
                return Err(Errno::EACCES);
            }
            true
        }
        MapFlags::MAP_PRIVATE => false,
        _ => return Err(Errno::EINVAL),
    };
    if file.check_readable().is_err() {
        return Err(Errno::EACCES);
    }
    // Only regular files have pages to map (`can_mmap_file`).
    let inode = match file.inode() {
        Ok(inode) if inode.file_type() == FileType::Regular => inode,
        _ => return Err(Errno::ENODEV),
    };
    if !shared {
        let file = file.clone();
        return Ok(Source::Private { file, offset });
    }
    // `memfd_check_seals_mmap`: a write-sealed file takes no writable shared mapping, and a
    // read-only one never becomes writable (`VM_MAYWRITE` cleared).
    crate::memfd::check_seals_mmap(inode, prot, flags)?;
    let sealed = inode
        .path
        .dentry
        .inode()
        .seals()
        .is_some_and(|seals| seals.intersects(WRITE_SEALS));
    let writable = writable && !sealed;
    let writer = match writable {
        true => Some(Writer::new(inode.path.dentry.inode().clone())?),
        false => None,
    };
    // The object reaches past the end of the mapping, however far past the end of the file
    // that is, so the host never faults where Linux would raise SIGBUS: the pages read zero.
    if let Some(pages) = inode.node().shared_pages(offset + pages * PAGE_SIZE)? {
        return Ok(Source::Mapped(Mapped {
            pages,
            offset,
            segment: None,
            writable,
            writer,
        }));
    }
    let shared = Shared::new(Object::File(file.clone()), offset, writable, writer);
    Ok(Source::Shared(shared))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growths_recorded_out_of_order() {
        // Two threads grow from 10 and then 12 pages, and record it in the other order.
        let mut space = AddressSpace::default();
        space.grown(12 * PAGE_SIZE, 1);
        space.grown(10 * PAGE_SIZE, 2);
        assert_eq!(space.base, Some(12));
        // The earlier growth fell below `base`, so it stays out of the allocator's hands.
        assert_eq!(space.mapped, [true]);
        space.grown(13 * PAGE_SIZE, 1);
        space.grown(15 * PAGE_SIZE, 1);
        // Page 14, grown by someone else, is never handed out.
        assert_eq!(space.mapped, [true, true, true, true]);
        space.unmap(13 * PAGE_SIZE, PAGE_SIZE);
        assert!(matches!(space.place(1), Ok(Placement::Reuse(a)) if a == 13 * PAGE_SIZE));
        assert!(matches!(space.place(1), Ok(Placement::Grow(1))));
    }
}
