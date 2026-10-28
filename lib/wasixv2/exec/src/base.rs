//! The frozen image under a linear memory, which fork children share copy-on-write.
//!
//! On Linux a process's memory starts as a private view of a file ([`Base`]): once
//! instantiation filled the memory (data segments, and whatever else it wrote), and before any
//! guest code runs, the pages it touched are written to a new memfd, which is sealed against
//! every change, and the memory is mapped over it with `MAP_PRIVATE | MAP_FIXED` at the same
//! address. The memory is from then on exactly "the file, plus the pages the process wrote
//! since", which a store makes its own private copy of, as with any private file mapping.
//!
//! A fork child maps the same file before it writes the parent's private pages
//! ([`crate::memory_image`]), so parent and children share the pages of the file, the module's
//! data segments, instead of each child holding a copy: Linux's copy-on-write fork, for the part
//! of memory the program started with. The file stays frozen, so what a mapping of it shows never
//! changes under any process; a page the parent writes after the fork becomes its own, and the
//! child's view of the file stays as it was.
//!
//! **Invariant.** Wherever the file backs a memory (`[0, len)`), the memory is a private view of
//! it: a range given back after a shared object ([`Base::clear`]) is mapped from the file again
//! and its data overwritten with zeros, never left as anonymous pages, which a child would see as
//! the file. Nothing may drop pages there with `MADV_DONTNEED` either: the file's content would
//! show again. Zeroing is a write.
//!
//! **Cost.** One sealed memfd per process image, shared by every fork child of it, and none for
//! an image whose memory holds nothing at instantiation. Instantiation writes its touched pages
//! once more, into the file; the pages of the memory it filled are freed by the remap, so the
//! process holds them once, as shared memory (`RssShmem`). Elsewhere (no pagemap, other hosts)
//! a memory has no base, and fork copies its pages as before.

#[cfg(all(target_os = "linux", feature = "sys"))]
pub(crate) use linux::Base;

#[cfg(all(target_os = "linux", feature = "sys"))]
mod linux {
    use std::{
        fs::File,
        io,
        ops::Range,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::FileExt,
        },
        sync::Arc,
    };

    use wasmer::{AsStoreRef, Memory, MemoryView};

    use crate::{
        memory_image::{for_chunks, is_zero},
        pagemap::{Page, Pagemap},
    };

    /// Bytes of a run of pages written to the file at once.
    const WRITE_RUN: usize = 1 << 20;

    /// The frozen file under the first [`Base::len`] bytes of a linear memory.
    pub(crate) struct Base {
        file: File,
        /// Bytes from address 0 the file backs: the size of the memory when it was frozen.
        len: u64,
        /// The runs of host pages of the file that hold data, in address order and apart; the
        /// rest of it reads zero.
        data: Vec<Range<u64>>,
    }

    impl Base {
        /// Freezes what instantiation put into `memory`, which no guest code has run on yet
        /// and no other thread reaches: its touched pages that hold anything go into a new
        /// sealed file, and the memory becomes a private view of the file.
        ///
        /// `Ok(None)` where it has no base: the host has no pagemap or no memfd for it (out of
        /// descriptors, say), or instantiation wrote nothing. An error only when the memory
        /// cannot be put back as it was after a failed remap; it is unusable then.
        pub(crate) fn freeze(
            memory: &Memory,
            store: &impl AsStoreRef,
        ) -> io::Result<Option<Arc<Self>>> {
            let Some(pagemap) = Pagemap::get() else {
                return Ok(None);
            };
            let view = memory.view(store);
            let base = match Self::fill(pagemap, &view) {
                Ok(Some(base)) => base,
                Ok(None) => return Ok(None),
                Err(err) => {
                    tracing::warn!(%err, "cannot freeze a guest memory's image; fork copies it");
                    return Ok(None);
                }
            };
            let ptr = view.data_ptr();
            // SAFETY: the start of the memory's own mapping, at least `len` bytes (its size
            // then), page aligned, with nothing referring into it; the file holds every byte of
            // it that is not zero.
            if let Err(err) = unsafe { base.map(ptr, 0..base.len) } {
                tracing::warn!(%err, "cannot map a guest memory over its image; fork copies it");
                // SAFETY: as above; the range is fresh zeroed pages after this.
                unsafe { wasixv2_pages::map_private_at(ptr, base.len as usize)? };
                base.copy_out(&view)?;
                return Ok(None);
            }
            Ok(Some(Arc::new(base)))
        }

        /// A new sealed file holding the pages of `view` that hold anything, if any does.
        fn fill(pagemap: &Pagemap, view: &MemoryView) -> io::Result<Option<Self>> {
            let mut data = Vec::<Range<u64>>::new();
            let mut page = vec![0; pagemap.page_size()];
            // A run of pages read from the memory, not written to the file yet.
            let mut run = (0, Vec::with_capacity(WRITE_RUN));
            let mut file = None;
            let len = view.data_size();
            let mut flush = |run: &mut (u64, Vec<u8>)| -> io::Result<()> {
                if run.1.is_empty() {
                    return Ok(());
                }
                let file = match &mut file {
                    Some(file) => file,
                    None => file.insert(create(len)?),
                };
                file.write_all_at(&run.1, run.0)?;
                run.0 += run.1.len() as u64;
                run.1.clear();
                Ok(())
            };
            for_chunks(pagemap, view, 0..len, |addr, size, kind| {
                if kind == Page::Untouched {
                    return Ok(());
                }
                let page = &mut page[..size as usize];
                view.read(addr, page).map_err(io::Error::other)?;
                if is_zero(page) {
                    return Ok(());
                }
                match data.last_mut() {
                    Some(last) if last.end == addr => last.end += size,
                    _ => data.push(addr..addr + size),
                }
                if run.0 + run.1.len() as u64 != addr || run.1.len() >= WRITE_RUN {
                    flush(&mut run)?;
                    run.0 = addr;
                }
                run.1.extend_from_slice(page);
                Ok(())
            })?;
            flush(&mut run)?;
            let Some(file) = file else {
                return Ok(None);
            };
            seal(&file)?;
            Ok(Some(Self { file, len, data }))
        }

        /// Bytes from address 0 the file backs.
        pub(crate) fn len(&self) -> u64 {
            self.len
        }

        /// The parts of `range` the file holds data in.
        pub(crate) fn data_in(&self, range: Range<u64>) -> impl Iterator<Item = Range<u64>> + '_ {
            // The runs are in order and apart: skip those that end before the range.
            let (start, end) = (range.start, range.end);
            let first = self.data.partition_point(|data| data.end <= start);
            self.data[first..]
                .iter()
                .take_while(move |data| data.start < end)
                .map(move |data| data.start.max(start)..data.end.min(end))
                .filter(|part| part.start < part.end)
        }

        /// Makes the memory at `ptr` a private view of the file: a fork child's, before the
        /// parent's own pages are written into it.
        ///
        /// # Safety
        ///
        /// `ptr` is the base of a linear memory of at least [`Base::len`] bytes whose contents
        /// may be replaced: nothing refers into it, and no other thread reaches it.
        pub(crate) unsafe fn map_memory(&self, ptr: *mut u8) -> io::Result<()> {
            // SAFETY: as the caller promises.
            unsafe { self.map(ptr, 0..self.len) }
        }

        /// Makes `range` of the memory at `ptr` fresh pages that read zero, where a shared
        /// object was: as a private view of the file inside it, with the file's data there
        /// overwritten, and anonymous pages past it ([`wasixv2_pages::map_private_at`]).
        ///
        /// # Safety
        ///
        /// `ptr` is the base of the linear memory this file backs, `range` lies inside it, page
        /// aligned, and its contents may be replaced: nothing refers into it, and the host
        /// writes there only once this returned.
        pub(crate) unsafe fn clear(&self, ptr: *mut u8, range: Range<u64>) -> io::Result<()> {
            let inside = range.start.min(self.len)..range.end.min(self.len);
            if inside.start < inside.end {
                // SAFETY: as the caller promises.
                unsafe { self.map(ptr, inside.clone())? };
                for data in self.data_in(inside) {
                    // SAFETY: inside the range, which is readable and writable now.
                    unsafe {
                        ptr.add(data.start as usize)
                            .write_bytes(0, (data.end - data.start) as usize)
                    };
                }
            }
            let past = range.start.max(self.len)..range.end;
            if past.start < past.end {
                // SAFETY: as the caller promises.
                let at = unsafe { ptr.add(past.start as usize) };
                // SAFETY: as the caller promises.
                unsafe { wasixv2_pages::map_private_at(at, (past.end - past.start) as usize)? };
            }
            Ok(())
        }

        /// Maps `range` of the file at the same offset of the memory at `ptr`, privately.
        ///
        /// # Safety
        ///
        /// As for [`Base::clear`].
        unsafe fn map(&self, ptr: *mut u8, range: Range<u64>) -> io::Result<()> {
            let len = (range.end - range.start) as usize;
            let offset = libc::off_t::try_from(range.start).map_err(io::Error::other)?;
            let prot = libc::PROT_READ | libc::PROT_WRITE;
            let flags = libc::MAP_PRIVATE | libc::MAP_FIXED;
            let fd = self.file.as_raw_fd();
            // SAFETY: as the caller promises; MAP_FIXED replaces the range in place, inside a
            // file whose size is sealed at `len`, so no access through it can raise SIGBUS.
            let at = unsafe { ptr.add(range.start as usize) };
            // SAFETY: as above.
            let mapped = unsafe { libc::mmap(at.cast(), len, prot, flags, fd, offset) };
            match mapped == libc::MAP_FAILED {
                true => Err(io::Error::last_os_error()),
                false => Ok(()),
            }
        }

        /// Writes the file's data into `view`, whose pages read zero.
        fn copy_out(&self, view: &MemoryView) -> io::Result<()> {
            let mut buf = Vec::new();
            for data in &self.data {
                buf.resize((data.end - data.start) as usize, 0);
                self.file.read_exact_at(&mut buf, data.start)?;
                view.write(data.start, &buf).map_err(io::Error::other)?;
            }
            Ok(())
        }
    }

    /// A new memfd of `len` bytes, all zero, that can be sealed.
    fn create(len: u64) -> io::Result<File> {
        let flags = libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING;
        // SAFETY: a NUL-terminated name and flags `memfd_create` knows.
        let fd = unsafe { libc::memfd_create(c"wasixv2-image".as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a descriptor just opened, owned by nothing else.
        let file = File::from(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) });
        file.set_len(len)?;
        Ok(file)
    }

    /// Seals `file` against every change of its size and content (`memfd_add_seals`), which
    /// private mappings of it, writable or not, are still allowed.
    fn seal(file: &File) -> io::Result<()> {
        let seals =
            libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
        // SAFETY: the file's own descriptor and a seal set `fcntl` knows.
        let ret = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) };
        match ret {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    #[cfg(test)]
    mod tests {
        use wasmer::{MemoryType, Store};

        use super::*;

        const WASM_PAGE: u64 = 65536;

        fn pages(memory: &Memory, store: &Store, addr: u64, count: usize) -> Vec<Page> {
            let pagemap = Pagemap::get().unwrap();
            let ptr = memory.view(store).data_ptr() as usize + addr as usize;
            pagemap.pages(ptr, count).unwrap()
        }

        #[test]
        fn a_memory_becomes_a_private_view_of_its_frozen_image() {
            let Some(pagemap) = Pagemap::get() else {
                eprintln!("skipped: this host has no usable /proc/self/pagemap");
                return;
            };
            let host_page = pagemap.page_size() as u64;
            let mut store = Store::default();
            let memory = Memory::new(&mut store, MemoryType::new(4, None, false)).unwrap();
            let view = memory.view(&store);
            // What instantiation filled: data segments, and a page of zeros.
            view.write(WASM_PAGE + 3, b"data segment").unwrap();
            view.write(3 * WASM_PAGE - 2, b"across").unwrap();
            view.write(2 * WASM_PAGE + host_page, &[0; 8]).unwrap();
            let before = view.copy_to_vec().unwrap();

            let base = Base::freeze(&memory, &store).unwrap().expect("a base");
            assert_eq!(base.len(), 4 * WASM_PAGE);
            let data: Vec<_> = base.data_in(0..base.len()).collect();
            let across = 3 * WASM_PAGE - host_page..3 * WASM_PAGE + host_page;
            assert_eq!(data, [WASM_PAGE..WASM_PAGE + host_page, across]);
            let view = memory.view(&store);
            assert!(view.copy_to_vec().unwrap() == before);
            // Read, the data is the file's page; nothing is the memory's own.
            assert_eq!(pages(&memory, &store, WASM_PAGE, 1), [Page::Shared]);

            // A store makes the page the memory's own; the file keeps its bytes.
            view.write(WASM_PAGE + 3, b"DATA").unwrap();
            assert_eq!(pages(&memory, &store, WASM_PAGE, 1), [Page::Private]);
            let mut buf = [0; 12];
            base.file.read_exact_at(&mut buf, WASM_PAGE + 3).unwrap();
            assert_eq!(&buf, b"data segment");
            // Nothing can change the file any more.
            assert!(base.file.write_all_at(b"x", 0).is_err());
            assert!(base.file.set_len(0).is_err());
        }

        #[test]
        fn data_in_clips_the_runs_to_the_range() {
            let base = Base {
                file: create(100).unwrap(),
                len: 100,
                data: vec![10..20, 30..40, 50..60],
            };
            let parts = |range| base.data_in(range).collect::<Vec<_>>();
            assert_eq!(parts(0..100), [10..20, 30..40, 50..60]);
            assert_eq!(parts(15..35), [15..20, 30..35]);
            assert!(parts(20..30).is_empty());
            assert_eq!(parts(39..51), [39..40, 50..51]);
            assert!(parts(60..100).is_empty());
        }

        #[test]
        fn a_memory_that_holds_nothing_has_no_base() {
            if Pagemap::get().is_none() {
                eprintln!("skipped: this host has no usable /proc/self/pagemap");
                return;
            }
            let mut store = Store::default();
            let memory = Memory::new(&mut store, MemoryType::new(2, None, false)).unwrap();
            memory.view(&store).write(WASM_PAGE, &[0; 64]).unwrap();
            assert!(Base::freeze(&memory, &store).unwrap().is_none());
        }

        #[test]
        fn a_cleared_range_reads_zero_and_stays_a_view_of_the_file() {
            if Pagemap::get().is_none() {
                eprintln!("skipped: this host has no usable /proc/self/pagemap");
                return;
            }
            let mut store = Store::default();
            let memory = Memory::new(&mut store, MemoryType::new(4, None, false)).unwrap();
            let view = memory.view(&store);
            view.write(WASM_PAGE + 3, b"data").unwrap();
            view.write(2 * WASM_PAGE, b"more data").unwrap();
            let base = Base::freeze(&memory, &store).unwrap().expect("a base");
            memory.grow(&mut store, 2).unwrap();
            let view = memory.view(&store);
            view.write(4 * WASM_PAGE, b"grown").unwrap();
            view.write(2 * WASM_PAGE, b"written").unwrap();

            // Across the file's data and past its end, as after a shared object there.
            let range = WASM_PAGE..5 * WASM_PAGE;
            // SAFETY: inside the memory, which nothing refers into.
            unsafe { base.clear(view.data_ptr(), range.clone()).unwrap() };
            // Where the file has no data its pages show, untouched; over its data, zeros written
            // (before anything reads them).
            assert_eq!(pages(&memory, &store, 3 * WASM_PAGE, 1), [Page::Untouched]);
            assert_eq!(pages(&memory, &store, 2 * WASM_PAGE, 1), [Page::Private]);
            assert_eq!(pages(&memory, &store, 4 * WASM_PAGE, 1), [Page::Untouched]);
            let bytes = view.copy_to_vec().unwrap();
            assert!(
                bytes[range.start as usize..range.end as usize]
                    .iter()
                    .all(|&b| b == 0)
            );
            view.write(WASM_PAGE, b"x").unwrap();
        }
    }
}

/// Other hosts have no base: a memory is never a view of a file there.
#[cfg(not(all(target_os = "linux", feature = "sys")))]
pub(crate) enum Base {}

/// [`Base::freeze`] where the host has bases; `None` elsewhere.
pub(crate) fn freeze(
    memory: &wasmer::Memory,
    store: &impl wasmer::AsStoreRef,
) -> std::io::Result<Option<std::sync::Arc<Base>>> {
    #[cfg(all(target_os = "linux", feature = "sys"))]
    return Base::freeze(memory, store);
    #[cfg(not(all(target_os = "linux", feature = "sys")))]
    {
        let _ = (memory, store);
        Ok(None)
    }
}
