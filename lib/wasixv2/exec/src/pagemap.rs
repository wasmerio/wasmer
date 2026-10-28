//! Which host pages of a mapping hold anything, from Linux's `/proc/self/pagemap`
//! (`Documentation/admin-guide/mm/pagemap.rst`; `fs/proc/task_mmu.c`, `pagemap_read`): one 64-bit
//! entry per host page, readable without privileges since Linux 4.2, which only hides the frame
//! numbers. A fork copies only the pages of linear memory that hold anything through it.
//!
//! What an entry says of a page of a private anonymous mapping, such as wasmer's linear memory:
//!
//! - **Neither present nor swapped:** no page table entry, so the page was never touched and reads
//!   zero. Nothing makes a touched page lose its entry with its content kept: reclaim, zswap and
//!   migration leave a swap or migration entry, which reads as swapped, and khugepaged's collapse
//!   or a split of a huge page keeps every byte present. Only `MADV_DONTNEED` and `munmap` drop
//!   pages, and both drop their content too.
//! - **Present or swapped:** it was written, or only read. A read maps the shared zero page (or the
//!   huge zero page), which `vm_normal_page` does not count as a normal page, so it is present
//!   but not marked as a file page: an entry cannot tell it from a written page, the content can.
//! - **File or shared (bit 61):** the page belongs to a file or to shared memory, whose content
//!   the page tables do not hold. Linear memory has such pages only where it shows a shared
//!   object, which a fork leaves out of its copy; a caller seeing one elsewhere falls back.

use std::{fs::File, io, os::unix::fs::FileExt, sync::OnceLock};

const PRESENT: u64 = 1 << 63;
const SWAPPED: u64 = 1 << 62;
const FILE_OR_SHARED: u64 = 1 << 61;

/// What `/proc/self/pagemap` says of one host page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    /// No page table entry: never touched since it was mapped, and reads zero.
    Untouched,
    /// A private anonymous page, in memory or swapped out: written, or read.
    Private,
    /// A page of a file or of shared memory.
    Shared,
}

impl Page {
    pub(crate) fn of(entry: u64) -> Self {
        if entry & FILE_OR_SHARED != 0 {
            Self::Shared
        } else if entry & (PRESENT | SWAPPED) != 0 {
            Self::Private
        } else {
            Self::Untouched
        }
    }
}

/// This process's `/proc/self/pagemap`, open for the life of the process.
pub(crate) struct Pagemap {
    file: File,
    page_size: usize,
}

impl Pagemap {
    /// The pagemap, if this host has one that works: opened and probed once. Containers without
    /// `/proc`, and kernels that emulate it without page tables, have none.
    pub(crate) fn get() -> Option<&'static Pagemap> {
        static PAGEMAP: OnceLock<Option<Pagemap>> = OnceLock::new();
        PAGEMAP
            .get_or_init(|| match Pagemap::open() {
                Ok(pagemap) => Some(pagemap),
                Err(err) => {
                    tracing::debug!(%err, "no usable /proc/self/pagemap; fork copies all memory");
                    None
                }
            })
            .as_ref()
    }

    fn open() -> io::Result<Pagemap> {
        // SAFETY: sysconf has no preconditions.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let page_size = usize::try_from(page_size).map_err(|_| io::Error::last_os_error())?;
        let pagemap = Pagemap {
            file: File::open("/proc/self/pagemap")?,
            page_size,
        };
        pagemap.probe()?;
        Ok(pagemap)
    }

    /// Checks that the pagemap tells a written page from an untouched one, on a mapping of two
    /// pages made for the purpose.
    fn probe(&self) -> io::Result<()> {
        let len = 2 * self.page_size;
        // SAFETY: a fresh private anonymous mapping, unmapped below and used for nothing else.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the first byte of the mapping above.
        unsafe { ptr.cast::<u8>().write_volatile(1) };
        let mut entries = [0; 2];
        let read = self.read(ptr as usize, &mut entries);
        // SAFETY: the mapping above, which nothing refers to any more.
        unsafe { libc::munmap(ptr, len) };
        read?;
        match entries.map(Page::of) {
            [Page::Private, Page::Untouched] => Ok(()),
            pages => Err(io::Error::other(format!(
                "the pagemap reports {pages:?} for a written and an untouched page"
            ))),
        }
    }

    /// The host page size the entries are counted in.
    pub(crate) fn page_size(&self) -> usize {
        self.page_size
    }

    /// Reads the entries of the host pages from `addr`, which is page aligned, into `entries`.
    fn read(&self, addr: usize, entries: &mut [u64]) -> io::Result<()> {
        let mut buf = vec![0u8; entries.len() * 8];
        let offset = (addr / self.page_size * 8) as u64;
        self.file.read_exact_at(&mut buf, offset)?;
        for (entry, bytes) in entries.iter_mut().zip(buf.chunks_exact(8)) {
            *entry = u64::from_ne_bytes(bytes.try_into().expect("8 bytes"));
        }
        Ok(())
    }

    /// What the pagemap says of the host pages from `addr`, which is page aligned.
    pub(crate) fn pages(&self, addr: usize, count: usize) -> io::Result<Vec<Page>> {
        let mut entries = vec![0; count];
        self.read(addr, &mut entries)?;
        Ok(entries.into_iter().map(Page::of).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mapping of `pages` host pages, unmapped when dropped.
    struct Mapping {
        ptr: *mut u8,
        len: usize,
    }

    impl Mapping {
        fn new(pagemap: &Pagemap, pages: usize, flags: libc::c_int) -> Self {
            let len = pages * pagemap.page_size();
            // SAFETY: a fresh anonymous mapping, owned by the returned value.
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    flags | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            assert_ne!(ptr, libc::MAP_FAILED);
            Self {
                ptr: ptr.cast(),
                len,
            }
        }

        fn write(&self, page: usize, size: usize) {
            // SAFETY: inside the mapping, which is writable.
            unsafe { self.ptr.add(page * size).write_volatile(7) };
        }

        fn read(&self, page: usize, size: usize) -> u8 {
            // SAFETY: inside the mapping, which is readable.
            unsafe { self.ptr.add(page * size).read_volatile() }
        }
    }

    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: the mapping this value owns.
            unsafe { libc::munmap(self.ptr.cast(), self.len) };
        }
    }

    fn pagemap() -> Option<&'static Pagemap> {
        let pagemap = Pagemap::get();
        if pagemap.is_none() {
            eprintln!("skipped: this host has no usable /proc/self/pagemap");
        }
        pagemap
    }

    #[test]
    fn written_pages_are_private_and_untouched_ones_are_not() {
        let Some(pagemap) = pagemap() else { return };
        let size = pagemap.page_size();
        let mapping = Mapping::new(pagemap, 6, libc::MAP_PRIVATE);
        mapping.write(1, size);
        mapping.write(4, size);
        let pages = pagemap.pages(mapping.ptr as usize, 6).unwrap();
        use Page::*;
        assert_eq!(
            pages,
            [Untouched, Private, Untouched, Untouched, Private, Untouched]
        );
    }

    #[test]
    fn a_page_only_read_is_private_too() {
        // The shared zero page is not a normal page, so it is not reported as a file page.
        let Some(pagemap) = pagemap() else { return };
        let size = pagemap.page_size();
        let mapping = Mapping::new(pagemap, 2, libc::MAP_PRIVATE);
        assert_eq!(mapping.read(0, size), 0);
        let pages = pagemap.pages(mapping.ptr as usize, 2).unwrap();
        assert_eq!(pages, [Page::Private, Page::Untouched]);
    }

    #[test]
    fn shared_memory_is_shared() {
        let Some(pagemap) = pagemap() else { return };
        let size = pagemap.page_size();
        let mapping = Mapping::new(pagemap, 2, libc::MAP_SHARED);
        mapping.write(0, size);
        let pages = pagemap.pages(mapping.ptr as usize, 2).unwrap();
        assert_eq!(pages[0], Page::Shared);
    }

    #[test]
    fn entries_are_classified_by_their_flags() {
        assert_eq!(Page::of(0), Page::Untouched);
        // Soft-dirty and exclusively-mapped alone mean nothing is there.
        assert_eq!(Page::of(1 << 55 | 1 << 56), Page::Untouched);
        assert_eq!(Page::of(PRESENT), Page::Private);
        assert_eq!(Page::of(SWAPPED | 0x1234), Page::Private);
        assert_eq!(Page::of(PRESENT | FILE_OR_SHARED), Page::Shared);
    }
}
