//! The copy of linear memory a fork child starts from.
//!
//! On Linux the copy holds only the pages of the parent's memory that hold anything: a fork
//! takes `/proc/self/pagemap` ([`crate::pagemap`]) for the host pages the parent touched, copies
//! those, and leaves out the ones that read zero, so the child commits what the parent wrote
//! rather than the whole memory, most of whose 8 MiB stack is never touched. Elsewhere, where the
//! pagemap does not work, and for memory that is not private and anonymous, it copies every byte.
//!
//! **Shared objects.** Ranges of the memory that show a shared object's pages are left out of a
//! sparse copy: the child's memory shows the same object there (`dup_mmap` copies no pages of a
//! shared mapping, `vma_needs_copy` in `mm/memory.c`). Their pages are the only ones of a linear
//! memory the pagemap reports as shared; anywhere else such a page makes the copy a full one.
//!
//! **Wiped ranges.** Ranges `MADV_WIPEONFORK` marked are left out of a sparse copy too, so the
//! child's pages there read zero, and a full copy holds zeros there (`dup_mmap` copies no pages
//! of a `VM_WIPEONFORK` area).
//!
//! **What the child sees.** As with a full copy, the forking thread copies while the parent's
//! other threads run, so each page of the child holds its parent's page as it was at some instant
//! during the fork, and a page another thread writes during the copy may come out torn. For an
//! untouched page that instant is when the pagemap was read, for a touched one when it was copied,
//! and the pagemap is read in chunks just before their pages are copied. A write that happened
//! before the fork, by any thread, is never missed: the page fault that backed its page set the
//! page table entry before the store completed, and the pagemap reads those entries under the
//! page table lock. Linux's own `copy_page_range` is atomic per page too, but no thread runs
//! during it, since `dup_mmap` holds the write lock of every mapping the faults need.
//!
//! **The child's own pages.** The child is a new instance, which instantiation already filled
//! with the module's data segments. Those pages, and any other the child holds outside the copied
//! runs, are cleared, since the parent's held zero there.
//!
//! **Over a base.** Where the parent's memory is a private view of a frozen file ([`Base`]), a
//! page it never wrote shows the file, whether it reads zero or holds a data segment, and the
//! pagemap reports such a page, once read, as a file page. The copy holds only the pages the
//! parent wrote, those that read zero too where the file holds data, and the child maps the same
//! file over its memory first, which also drops the pages its instantiation filled: parent and
//! child share the file's pages. Wiped ranges where the file holds data are zeroed in the child.

use std::{io, ops::Range, sync::Arc};

#[cfg(all(target_os = "linux", feature = "sys"))]
use wasmer::MemoryView;
use wasmer::{AsStoreRef, Memory, MemoryAccessError};

use crate::base::Base;
#[cfg(all(target_os = "linux", feature = "sys"))]
use crate::pagemap::{Page, Pagemap};

/// A copy of linear memory.
pub(crate) enum MemoryImage {
    /// Every byte of it.
    Full(Vec<u8>),
    /// Only the pages that hold anything.
    #[cfg(all(target_os = "linux", feature = "sys"))]
    Sparse(Sparse),
}

impl MemoryImage {
    /// Copies `memory`, whose frozen image is `base`, sparsely where the host can tell which
    /// pages hold anything, with `wiped` (the child's `MADV_WIPEONFORK` ranges) reading zero. A
    /// sparse copy leaves out `shared`, the ranges that show shared objects, and `wiped`, and
    /// shares `base` with the child; a full one holds every byte but those of `wiped`. Each list
    /// is in address order, and no range of one overlaps one of the other.
    pub(crate) fn capture(
        memory: &Memory,
        store: &impl AsStoreRef,
        (shared, wiped): (&[Range<u64>], &[Range<u64>]),
        base: Option<&Arc<Base>>,
    ) -> Result<Self, MemoryAccessError> {
        let view = memory.view(store);
        #[cfg(not(all(target_os = "linux", feature = "sys")))]
        let _ = (shared, base);
        #[cfg(all(target_os = "linux", feature = "sys"))]
        if let Some(pagemap) = Pagemap::get() {
            let mut skipped = [shared, wiped].concat();
            skipped.sort_by_key(|range| range.start);
            match Sparse::capture(pagemap, &view, &skipped, base, wiped) {
                Ok(sparse) => return Ok(Self::Sparse(sparse)),
                Err(err) => tracing::warn!(%err, "cannot copy guest memory sparsely; copying all"),
            }
        }
        let mut bytes = view.copy_to_vec()?;
        for range in wiped {
            let end = (range.end as usize).min(bytes.len());
            let start = (range.start as usize).min(end);
            bytes[start..end].fill(0);
        }
        Ok(Self::Full(bytes))
    }

    /// The size of the memory copied, in bytes.
    pub(crate) fn size(&self) -> u64 {
        match self {
            Self::Full(bytes) => bytes.len() as u64,
            #[cfg(all(target_os = "linux", feature = "sys"))]
            Self::Sparse(sparse) => sparse.size,
        }
    }

    /// The frozen image the child's memory is a view of, after [`Self::restore`].
    pub(crate) fn base(&self) -> Option<&Arc<Base>> {
        match self {
            Self::Full(_) => None,
            #[cfg(all(target_os = "linux", feature = "sys"))]
            Self::Sparse(sparse) => sparse.base.as_ref(),
        }
    }

    /// Makes `memory`, a new instance's of at least [`Self::size`] bytes whose low guard is not
    /// protected yet and which no other thread reaches, hold the copy.
    pub(crate) fn restore(&self, memory: &Memory, store: &impl AsStoreRef) -> io::Result<()> {
        let view = memory.view(store);
        match self {
            Self::Full(bytes) => view.write(0, bytes).map_err(io::Error::other),
            #[cfg(all(target_os = "linux", feature = "sys"))]
            Self::Sparse(sparse) => sparse.restore(&view),
        }
    }
}

/// Host pages of the pagemap read at once: 32 KiB of entries, for 16 MiB of 4 KiB pages.
#[cfg(all(target_os = "linux", feature = "sys"))]
const CHUNK: usize = 4096;

/// The pages of a memory that hold anything, all others reading zero.
#[cfg(all(target_os = "linux", feature = "sys"))]
pub(crate) struct Sparse {
    size: u64,
    /// Runs of host pages, as `(address, length)`, in address order and apart.
    runs: Vec<(u64, u64)>,
    /// The runs' bytes, end to end.
    bytes: Vec<u8>,
    /// The frozen image under the memory, which shows wherever there is no run.
    base: Option<Arc<Base>>,
    /// The ranges of `base` that hold data and read zero in the child: wiped ones.
    zeroed: Vec<Range<u64>>,
}

#[cfg(all(target_os = "linux", feature = "sys"))]
impl Sparse {
    /// Copies the pages of `view` that hold anything, outside `skipped`; where the memory is a
    /// private view of `base`, those the parent wrote. Fails on memory that is neither private
    /// anonymous memory nor a view of `base` there, whose untouched pages need not read zero.
    /// `wiped` read zero in the child.
    fn capture(
        pagemap: &Pagemap,
        view: &MemoryView,
        skipped: &[Range<u64>],
        base: Option<&Arc<Base>>,
        wiped: &[Range<u64>],
    ) -> io::Result<Self> {
        let mut sparse = Self {
            size: view.data_size(),
            runs: Vec::new(),
            bytes: Vec::new(),
            base: base.cloned(),
            zeroed: Vec::new(),
        };
        let based = base.map_or(0, |base| base.len());
        for range in outside(sparse.size, skipped) {
            for_chunks(pagemap, view, range, |addr, len, page| match page {
                Page::Untouched => Ok(()),
                // The base's own page, which the parent read but never wrote.
                Page::Shared if addr < based => Ok(()),
                Page::Private => {
                    // Over the base's data, a page of zeros differs from what the child sees.
                    let page = addr..addr + len;
                    let data = base.is_some_and(|base| base.data_in(page).next().is_some());
                    sparse.push(view, addr, len, data)
                }
                Page::Shared => Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "the memory is not private anonymous memory",
                )),
            })?;
        }
        if let Some(base) = base {
            let zeroed = wiped.iter().flat_map(|range| base.data_in(range.clone()));
            sparse.zeroed = zeroed.collect();
        }
        Ok(sparse)
    }

    /// Copies the page at `addr`, unless it reads zero, as a page the guest only read does, and
    /// `keep_zero` is false.
    fn push(&mut self, view: &MemoryView, addr: u64, len: u64, keep_zero: bool) -> io::Result<()> {
        let at = self.bytes.len();
        self.bytes.resize(at + len as usize, 0);
        view.read(addr, &mut self.bytes[at..])
            .map_err(io::Error::other)?;
        if !keep_zero && is_zero(&self.bytes[at..]) {
            self.bytes.truncate(at);
            return Ok(());
        }
        match self.runs.last_mut() {
            Some((start, run)) if *start + *run == addr => *run += len,
            _ => self.runs.push((addr, len)),
        }
        Ok(())
    }

    fn restore(&self, view: &MemoryView) -> io::Result<()> {
        // The base shows in the gaps it covers, as it does in the parent.
        let mut based = 0;
        if let Some(base) = &self.base {
            // SAFETY: the child's memory, of the parent's size at least, so at least the base's;
            // no guest code ran on it, nothing refers into it, and no other thread reaches it
            // (`MemoryImage::restore`).
            unsafe { base.map_memory(view.data_ptr())? };
            based = base.len();
        }
        let gaps = self.gaps().map(|gap| gap.start.max(based)..gap.end);
        for gap in gaps.filter(|gap| gap.start < gap.end) {
            clear(view, gap).map_err(io::Error::other)?;
        }
        for range in &self.zeroed {
            zero(view, range.clone()).map_err(io::Error::other)?;
        }
        let mut from = 0;
        for &(addr, len) in &self.runs {
            let to = from + len as usize;
            view.write(addr, &self.bytes[from..to])
                .map_err(io::Error::other)?;
            from = to;
        }
        Ok(())
    }

    /// The ranges of the memory between the runs.
    fn gaps(&self) -> impl Iterator<Item = Range<u64>> + '_ {
        let ends = self.runs.iter().map(|&(addr, _)| addr).chain([self.size]);
        let starts = [0].into_iter().chain(self.runs.iter().map(|&(a, l)| a + l));
        starts
            .zip(ends)
            .filter(|(start, end)| start < end)
            .map(|(s, e)| s..e)
    }
}

/// The parts of a memory of `size` bytes outside `ranges`, which are in address order and apart.
#[cfg(all(target_os = "linux", feature = "sys"))]
fn outside(size: u64, ranges: &[Range<u64>]) -> Vec<Range<u64>> {
    let mut parts = Vec::with_capacity(ranges.len() + 1);
    let mut at = 0;
    for range in ranges {
        if range.start > at {
            parts.push(at..range.start.min(size));
        }
        at = at.max(range.end);
    }
    if at < size {
        parts.push(at..size);
    }
    parts
}

/// Calls `f` with each host page of `range` of `view` (page aligned) and what the pagemap says
/// of it, reading the pagemap a chunk at a time just before the chunk's pages.
#[cfg(all(target_os = "linux", feature = "sys"))]
pub(crate) fn for_chunks(
    pagemap: &Pagemap,
    view: &MemoryView,
    range: Range<u64>,
    mut f: impl FnMut(u64, u64, Page) -> io::Result<()>,
) -> io::Result<()> {
    let page = pagemap.page_size() as u64;
    let base = view.data_ptr() as usize;
    let mut addr = range.start;
    while addr < range.end {
        let count = (range.end - addr).div_ceil(page).min(CHUNK as u64) as usize;
        for kind in pagemap.pages(base + addr as usize, count)? {
            let len = page.min(range.end - addr);
            f(addr, len, kind)?;
            addr += len;
        }
    }
    Ok(())
}

/// Makes `range` of a new instance's memory read zero: the pages instantiation filled, and any
/// other it touched. Every page, where the pagemap cannot say which.
#[cfg(all(target_os = "linux", feature = "sys"))]
fn clear(view: &MemoryView, range: Range<u64>) -> Result<(), MemoryAccessError> {
    let mut touched = Vec::new();
    let scanned = Pagemap::get().map(|pagemap| {
        for_chunks(pagemap, view, range.clone(), |addr, len, page| {
            if page != Page::Untouched {
                touched.push(addr..addr + len);
            }
            Ok(())
        })
    });
    match scanned {
        Some(Ok(())) => touched.into_iter().try_for_each(|range| zero(view, range)),
        _ => zero(view, range),
    }
}

#[cfg(all(target_os = "linux", feature = "sys"))]
fn zero(view: &MemoryView, range: Range<u64>) -> Result<(), MemoryAccessError> {
    static ZEROS: [u8; 1 << 16] = [0; 1 << 16];
    let mut addr = range.start;
    while addr < range.end {
        let len = (range.end - addr).min(ZEROS.len() as u64);
        view.write(addr, &ZEROS[..len as usize])?;
        addr += len;
    }
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "sys"))]
pub(crate) fn is_zero(bytes: &[u8]) -> bool {
    // Folding a fixed-size block lets the compiler vectorise it.
    let (blocks, rest) = bytes.as_chunks::<64>();
    blocks
        .iter()
        .all(|block| block.iter().fold(0, |acc, &byte| acc | byte) == 0)
        && rest.iter().all(|&byte| byte == 0)
}

#[cfg(all(test, target_os = "linux", feature = "sys"))]
mod tests {
    use wasmer::{MemoryType, Store};

    use super::*;

    const WASM_PAGE: u64 = 65536;

    fn memory(store: &mut Store, pages: u32) -> Memory {
        Memory::new(store, MemoryType::new(pages, Some(pages), false)).unwrap()
    }

    #[test]
    fn a_fork_copies_only_the_pages_that_hold_anything() {
        let Some(pagemap) = Pagemap::get() else {
            eprintln!("skipped: this host has no usable /proc/self/pagemap");
            return;
        };
        let host_page = pagemap.page_size() as u64;
        let mut store = Store::default();
        let parent = memory(&mut store, 8);
        let view = parent.view(&store);
        view.write(3, b"low").unwrap();
        view.write(WASM_PAGE + 100, b"heap").unwrap();
        view.write(5 * WASM_PAGE - 2, b"across").unwrap();
        // A page only read, and a page written with zeros, hold nothing.
        view.read_u8(6 * WASM_PAGE).unwrap();
        view.write(7 * WASM_PAGE, &[0; 16]).unwrap();
        let MemoryImage::Sparse(sparse) =
            MemoryImage::capture(&parent, &store, (&[], &[]), None).unwrap()
        else {
            panic!("expected a sparse copy");
        };
        assert_eq!(sparse.size, 8 * WASM_PAGE);
        let runs = [
            (0, host_page),
            (WASM_PAGE, host_page),
            (5 * WASM_PAGE - host_page, 2 * host_page),
        ];
        assert_eq!(sparse.runs, runs);
        assert_eq!(sparse.bytes.len() as u64, 4 * host_page);

        // The child's instantiation filled pages of its own, inside and outside the runs.
        let child = memory(&mut store, 8);
        let view = child.view(&store);
        view.write(10, b"data segment").unwrap();
        view.write(3 * WASM_PAGE, b"data segment").unwrap();
        view.write(7 * WASM_PAGE + 5, b"data segment").unwrap();
        MemoryImage::Sparse(sparse).restore(&child, &store).unwrap();
        // The child did not commit the pages the parent left untouched (before the comparison
        // below reads them).
        let base = view.data_ptr() as usize;
        let pages = pagemap.pages(base + 2 * WASM_PAGE as usize, 1).unwrap();
        assert_eq!(pages, [Page::Untouched]);
        let parent_bytes = parent.view(&store).copy_to_vec().unwrap();
        assert!(view.copy_to_vec().unwrap() == parent_bytes);
    }

    #[test]
    fn a_fork_leaves_out_shared_objects_and_the_child_maps_them_again() {
        use wasixv2_pages::{Memfd, SharedPages};

        use crate::memory::{LOW_GUARD, ReadOnly, map_shared};

        let Some(pagemap) = Pagemap::get() else {
            eprintln!("skipped: this host has no usable /proc/self/pagemap");
            return;
        };
        let host_page = pagemap.page_size() as u64;
        let mut store = Store::default();
        let parent = memory(&mut store, 8);
        let pages = Memfd::new(2 * WASM_PAGE).unwrap();
        let (at, len) = (4 * WASM_PAGE, 2 * WASM_PAGE);
        let range = (at as u32, len);
        let read_only = ReadOnly::default();
        map_shared(
            &parent,
            (LOW_GUARD, &read_only, None),
            &store,
            range,
            &pages,
            0,
            true,
        )
        .unwrap();
        let view = parent.view(&store);
        view.write(WASM_PAGE, b"private").unwrap();
        view.write(at + 10, b"shared").unwrap();

        // Without the range, the shared pages make the copy a full one, as before.
        let image = MemoryImage::capture(&parent, &store, (&[], &[]), None).unwrap();
        assert!(matches!(image, MemoryImage::Full(_)));
        let shared = Range {
            start: at,
            end: at + len,
        };
        let image = MemoryImage::capture(&parent, &store, (&[shared], &[]), None).unwrap();
        let MemoryImage::Sparse(sparse) = &image else {
            panic!("expected a sparse copy");
        };
        assert_eq!(sparse.runs, [(WASM_PAGE, host_page)]);

        let child = memory(&mut store, 8);
        image.restore(&child, &store).unwrap();
        let read_only = ReadOnly::default();
        map_shared(
            &child,
            (LOW_GUARD, &read_only, None),
            &store,
            range,
            &pages,
            0,
            true,
        )
        .unwrap();
        let view = child.view(&store);
        let mut buf = [0; 7];
        view.read(WASM_PAGE, &mut buf).unwrap();
        assert_eq!(&buf, b"private");
        view.read(at + 10, &mut buf[..6]).unwrap();
        assert_eq!(&buf[..6], b"shared");
        // Parent and child see each other's stores.
        view.write(at + WASM_PAGE, b"child").unwrap();
        parent
            .view(&store)
            .read(at + WASM_PAGE, &mut buf[..5])
            .unwrap();
        assert_eq!(&buf[..5], b"child");
        assert!(pages.allocated() >= host_page);
    }

    #[test]
    fn a_fork_zeroes_the_wipe_on_fork_ranges_in_the_child() {
        use wasixv2_pages::Memfd;

        use crate::memory::{LOW_GUARD, ReadOnly, map_shared};

        if Pagemap::get().is_none() {
            eprintln!("skipped: this host has no usable /proc/self/pagemap");
            return;
        }
        let mut store = Store::default();
        let parent = memory(&mut store, 8);
        let view = parent.view(&store);
        view.write(WASM_PAGE, b"kept").unwrap();
        view.write(2 * WASM_PAGE + 7, b"wiped").unwrap();
        view.write(4 * WASM_PAGE - 1, b"!").unwrap();
        let wiped = [Range {
            start: 2 * WASM_PAGE,
            end: 4 * WASM_PAGE,
        }];
        let check = |image: MemoryImage, store: &mut Store| {
            let child = memory(store, 8);
            // Instantiation's data segment, inside the wiped range.
            child.view(store).write(3 * WASM_PAGE, b"data").unwrap();
            image.restore(&child, store).unwrap();
            let bytes = child.view(store).copy_to_vec().unwrap();
            assert_eq!(&bytes[WASM_PAGE as usize..][..4], b"kept");
            let range = 2 * WASM_PAGE as usize..4 * WASM_PAGE as usize;
            assert!(bytes[range].iter().all(|&byte| byte == 0));
        };

        // Sparse: the range is left out.
        let image = MemoryImage::capture(&parent, &store, (&[], &wiped), None).unwrap();
        let MemoryImage::Sparse(sparse) = &image else {
            panic!("expected a sparse copy");
        };
        assert!(
            sparse
                .runs
                .iter()
                .all(|&(addr, len)| { addr + len <= wiped[0].start || addr >= wiped[0].end })
        );
        check(image, &mut store);

        // Full, as when a shared object the copy was not told about is mapped: zeros there.
        let pages = Memfd::new(WASM_PAGE).unwrap();
        let read_only = ReadOnly::default();
        let range = (6 * WASM_PAGE as u32, WASM_PAGE);
        map_shared(
            &parent,
            (LOW_GUARD, &read_only, None),
            &store,
            range,
            &pages,
            0,
            true,
        )
        .unwrap();
        parent.view(&store).write(6 * WASM_PAGE, b"shared").unwrap();
        let image = MemoryImage::capture(&parent, &store, (&[], &wiped), None).unwrap();
        assert!(matches!(image, MemoryImage::Full(_)));
        check(image, &mut store);
        // The parent keeps its data.
        let mut buf = [0; 5];
        parent
            .view(&store)
            .read(2 * WASM_PAGE + 7, &mut buf)
            .unwrap();
        assert_eq!(&buf, b"wiped");
    }

    #[test]
    fn a_fork_child_shares_its_parents_frozen_image() {
        let Some(pagemap) = Pagemap::get() else {
            eprintln!("skipped: this host has no usable /proc/self/pagemap");
            return;
        };
        let host_page = pagemap.page_size() as u64;
        let mut store = Store::default();
        // The module's data segments, in parent and child alike.
        let instantiate = |store: &mut Store| {
            let memory = memory(store, 8);
            let view = memory.view(store);
            for page in 1..4 {
                view.write(page * WASM_PAGE, b"data segment").unwrap();
            }
            memory
        };
        let parent = instantiate(&mut store);
        let base = crate::base::freeze(&parent, &store)
            .unwrap()
            .expect("a base");
        let view = parent.view(&store);
        // The parent changes a data page, zeroes another, and fills a heap page; it reads the
        // third data page, which stays the file's.
        view.write(WASM_PAGE + 4, b"SEG").unwrap();
        view.write(2 * WASM_PAGE, &[0; 12]).unwrap();
        view.write(5 * WASM_PAGE, b"heap").unwrap();
        view.read_u8(3 * WASM_PAGE).unwrap();
        let wiped = [Range {
            start: 6 * WASM_PAGE,
            end: 7 * WASM_PAGE,
        }];
        let image = MemoryImage::capture(&parent, &store, (&[], &wiped), Some(&base)).unwrap();
        let MemoryImage::Sparse(sparse) = &image else {
            panic!("expected a sparse copy");
        };
        // The zeroed data page is copied; the read one is not.
        let runs = [
            (WASM_PAGE, host_page),
            (2 * WASM_PAGE, host_page),
            (5 * WASM_PAGE, host_page),
        ];
        assert_eq!(sparse.runs, runs);

        let child = instantiate(&mut store);
        child.view(&store).write(7 * WASM_PAGE, b"more").unwrap();
        image.restore(&child, &store).unwrap();
        assert!(Arc::ptr_eq(image.base().unwrap(), &base));
        // The child's pages its instantiation filled are gone (before anything reads them);
        // its data page, once read, is the file's, as the parent's is.
        let view = child.view(&store);
        let ptr = view.data_ptr() as usize;
        let page = |addr: u64| pagemap.pages(ptr + addr as usize, 1).unwrap()[0];
        assert_eq!(page(3 * WASM_PAGE), Page::Untouched);
        assert_eq!(page(7 * WASM_PAGE), Page::Untouched);
        let parent_bytes = parent.view(&store).copy_to_vec().unwrap();
        assert!(view.copy_to_vec().unwrap() == parent_bytes);
        assert_eq!(page(3 * WASM_PAGE), Page::Shared);
        // What either writes after the fork stays its own.
        parent.view(&store).write(3 * WASM_PAGE, b"parent").unwrap();
        view.write(3 * WASM_PAGE + 6, b"child").unwrap();
        let mut buf = [0; 12];
        view.read(3 * WASM_PAGE, &mut buf).unwrap();
        assert_eq!(&buf, b"data schildt");
        parent.view(&store).read(3 * WASM_PAGE, &mut buf).unwrap();
        assert_eq!(&buf, b"parentegment");
    }

    #[test]
    fn a_fork_over_a_base_zeroes_the_wiped_data_in_the_child() {
        if Pagemap::get().is_none() {
            eprintln!("skipped: this host has no usable /proc/self/pagemap");
            return;
        }
        let mut store = Store::default();
        let parent = memory(&mut store, 4);
        parent.view(&store).write(WASM_PAGE, b"wiped data").unwrap();
        parent
            .view(&store)
            .write(2 * WASM_PAGE, b"kept data")
            .unwrap();
        let base = crate::base::freeze(&parent, &store)
            .unwrap()
            .expect("a base");
        let wiped = [Range {
            start: WASM_PAGE,
            end: 2 * WASM_PAGE,
        }];
        let image = MemoryImage::capture(&parent, &store, (&[], &wiped), Some(&base)).unwrap();
        assert!(matches!(image, MemoryImage::Sparse(_)));
        let child = memory(&mut store, 4);
        image.restore(&child, &store).unwrap();
        let bytes = child.view(&store).copy_to_vec().unwrap();
        let range = WASM_PAGE as usize..2 * WASM_PAGE as usize;
        assert!(bytes[range].iter().all(|&byte| byte == 0));
        assert_eq!(&bytes[2 * WASM_PAGE as usize..][..9], b"kept data");
    }

    #[test]
    fn a_shared_object_unmapped_over_a_base_reads_zero_in_a_fork_child() {
        use wasixv2_pages::Memfd;

        use crate::memory::{LOW_GUARD, ReadOnly, map_shared, unmap_shared};

        if Pagemap::get().is_none() {
            eprintln!("skipped: this host has no usable /proc/self/pagemap");
            return;
        }
        let mut store = Store::default();
        let parent = memory(&mut store, 4);
        parent.view(&store).write(2 * WASM_PAGE, b"data").unwrap();
        let base = crate::base::freeze(&parent, &store)
            .unwrap()
            .expect("a base");
        let pages = Memfd::new(WASM_PAGE).unwrap();
        let read_only = ReadOnly::default();
        let layout = (LOW_GUARD, &read_only, Some(&*base));
        let range = (2 * WASM_PAGE as u32, WASM_PAGE);
        map_shared(&parent, layout, &store, range, &pages, 0, true).unwrap();
        parent.view(&store).write(2 * WASM_PAGE, b"shared").unwrap();
        unmap_shared(&parent, layout, &store, range.0, range.1).unwrap();
        let image = MemoryImage::capture(&parent, &store, (&[], &[]), Some(&base)).unwrap();
        assert!(matches!(image, MemoryImage::Sparse(_)));
        let child = memory(&mut store, 4);
        image.restore(&child, &store).unwrap();
        for memory in [&parent, &child] {
            let bytes = memory.view(&store).copy_to_vec().unwrap();
            let range = 2 * WASM_PAGE as usize..3 * WASM_PAGE as usize;
            assert!(bytes[range].iter().all(|&byte| byte == 0));
        }
    }

    #[test]
    fn a_full_copy_restores_every_byte() {
        let mut store = Store::default();
        let parent = memory(&mut store, 2);
        parent.view(&store).write(WASM_PAGE + 1, b"x").unwrap();
        let image = MemoryImage::Full(parent.view(&store).copy_to_vec().unwrap());
        assert_eq!(image.size(), 2 * WASM_PAGE);
        let child = memory(&mut store, 2);
        child.view(&store).write(0, b"data segment").unwrap();
        image.restore(&child, &store).unwrap();
        let (parent, child) = (parent.view(&store), child.view(&store));
        assert!(child.copy_to_vec().unwrap() == parent.copy_to_vec().unwrap());
    }

    #[test]
    fn gaps_lie_between_the_runs() {
        let sparse = |runs: Vec<(u64, u64)>| Sparse {
            size: 100,
            runs,
            bytes: Vec::new(),
            base: None,
            zeroed: Vec::new(),
        };
        let gaps = |s: &Sparse| s.gaps().collect::<Vec<_>>();
        assert_eq!(gaps(&sparse(vec![])), vec![0..100]);
        assert_eq!(gaps(&sparse(vec![(0, 10), (50, 50)])), vec![10..50]);
        assert_eq!(
            gaps(&sparse(vec![(10, 10), (30, 10)])),
            [0..10, 20..30, 40..100]
        );
    }

    #[test]
    fn zero_means_every_byte() {
        assert!(is_zero(&[0; 4096]));
        let mut page = [0; 4100];
        page[4099] = 1;
        assert!(!is_zero(&page));
        page[4099] = 0;
        page[70] = 1;
        assert!(!is_zero(&page));
    }
}
