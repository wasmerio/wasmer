//! Placing an object's pages inside a host mapping, such as a linear memory's reservation, and
//! taking them out again.

use std::{io, os::fd::AsRawFd};

use crate::SharedPages;

/// The host's page size.
fn host_page_size() -> usize {
    // SAFETY: sysconf has no preconditions.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(size).unwrap_or(4096)
}

fn invalid() -> io::Error {
    io::Error::from_raw_os_error(libc::EINVAL)
}

/// Maps `len` bytes of `pages` from `offset` at `addr`, readable, and writable with `writable`,
/// replacing what was mapped there (`mmap` with `MAP_SHARED | MAP_FIXED`): from then on the range
/// shows the object, as every other mapping of it does. A store into a range mapped without
/// `writable` raises `SIGSEGV`, while the object's other mappings may still change its bytes.
///
/// Fails with `EINVAL` when the range or the offset is not page aligned or the range reaches past
/// the object's size, so no access through it can raise `SIGBUS`, and with
/// [`io::ErrorKind::Unsupported`] for an object the host cannot map. When the host fails the
/// `mmap` itself, the range is left as fresh zeroed private pages, as [`map_private_at`] makes
/// them.
///
/// # Safety
///
/// `addr..addr + len` must lie inside a mapping the caller owns, readable and writable, whose
/// contents may be replaced: nothing may hold a Rust reference into it. Without `writable`,
/// nothing may store into the range until it is mapped again.
pub unsafe fn map_at(
    addr: *mut u8,
    len: usize,
    pages: &dyn SharedPages,
    offset: u64,
    writable: bool,
) -> io::Result<()> {
    let fd = pages
        .fd()
        .ok_or_else(|| io::Error::from(io::ErrorKind::Unsupported))?;
    let page = host_page_size();
    let end = offset.checked_add(len as u64);
    if !(addr as usize).is_multiple_of(page)
        || !len.is_multiple_of(page)
        || !offset.is_multiple_of(page as u64)
        || end.is_none_or(|end| end > pages.size())
    {
        return Err(invalid());
    }
    let offset = libc::off_t::try_from(offset).map_err(|_| invalid())?;
    let prot = match writable {
        true => libc::PROT_READ | libc::PROT_WRITE,
        false => libc::PROT_READ,
    };
    let flags = libc::MAP_SHARED | libc::MAP_FIXED;
    // SAFETY: the caller owns the range and lets its contents go; MAP_FIXED replaces it in place.
    let ptr = unsafe { libc::mmap(addr.cast(), len, prot, flags, fd.as_raw_fd(), offset) };
    if ptr == libc::MAP_FAILED {
        let err = io::Error::last_os_error();
        // A failed MAP_FIXED may have unmapped the range already; keep it backed.
        // SAFETY: as above.
        let _ = unsafe { map_private_at(addr, len) };
        return Err(err);
    }
    Ok(())
}

/// Replaces `addr..addr + len` with fresh private anonymous pages, readable, writable and zero,
/// as the rest of a linear memory is: what [`map_at`] placed there is gone from the range.
///
/// # Safety
///
/// As for [`map_at`].
pub unsafe fn map_private_at(addr: *mut u8, len: usize) -> io::Result<()> {
    let page = host_page_size();
    if !(addr as usize).is_multiple_of(page) || !len.is_multiple_of(page) {
        return Err(invalid());
    }
    let prot = libc::PROT_READ | libc::PROT_WRITE;
    let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED;
    // SAFETY: the caller owns the range and lets its contents go; MAP_FIXED replaces it in place.
    let ptr = unsafe { libc::mmap(addr.cast(), len, prot, flags, -1, 0) };
    match ptr == libc::MAP_FAILED {
        true => Err(io::Error::last_os_error()),
        false => Ok(()),
    }
}

/// Drops the pages of `addr..addr + len`, a range of a private anonymous mapping, so it reads
/// zero again and holds no host memory until it is written (`MADV_DONTNEED`); a range nothing
/// wrote stays untouched. Unlike [`map_private_at`] the mapping itself stays as it is.
///
/// Only Linux drops private anonymous pages that way; [`io::ErrorKind::Unsupported`] elsewhere,
/// where `MADV_DONTNEED` may keep the contents. `EINVAL` when the range is not page aligned.
///
/// # Safety
///
/// `addr..addr + len` must lie inside a private anonymous mapping the caller owns, whose contents
/// may be replaced: nothing may hold a Rust reference into it. Inside a private mapping of a
/// file, the file's contents would show instead of zeros.
pub unsafe fn drop_private(addr: *mut u8, len: usize) -> io::Result<()> {
    let page = host_page_size();
    if !(addr as usize).is_multiple_of(page) || !len.is_multiple_of(page) {
        return Err(invalid());
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: the caller owns the range and lets its contents go.
        let ret = unsafe { libc::madvise(addr.cast(), len, libc::MADV_DONTNEED) };
        match ret {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "android", target_os = "macos")
))]
mod tests {
    use super::*;
    use crate::Memfd;

    const PAGE: usize = 65536;

    /// A private anonymous mapping of `pages` pages, unmapped when dropped.
    struct Region {
        ptr: *mut u8,
        len: usize,
    }

    impl Region {
        fn new(pages: usize) -> Self {
            let len = pages * PAGE;
            let prot = libc::PROT_READ | libc::PROT_WRITE;
            let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS;
            // SAFETY: a fresh mapping, owned by the returned value.
            let ptr = unsafe { libc::mmap(std::ptr::null_mut(), len, prot, flags, -1, 0) };
            assert_ne!(ptr, libc::MAP_FAILED);
            Self {
                ptr: ptr.cast(),
                len,
            }
        }

        fn page(&self, page: usize) -> *mut u8 {
            assert!(page * PAGE < self.len);
            // SAFETY: inside the mapping.
            unsafe { self.ptr.add(page * PAGE) }
        }

        fn get(&self, page: usize, at: usize) -> u8 {
            // SAFETY: inside the mapping, which stays readable.
            unsafe { self.page(page).add(at).read_volatile() }
        }

        fn set(&self, page: usize, at: usize, value: u8) {
            // SAFETY: inside the mapping, which stays writable.
            unsafe { self.page(page).add(at).write_volatile(value) }
        }
    }

    impl Drop for Region {
        fn drop(&mut self) {
            // SAFETY: the mapping this value owns.
            unsafe { libc::munmap(self.ptr.cast(), self.len) };
        }
    }

    #[test]
    fn two_places_mapping_one_object_see_the_same_bytes() {
        let pages = Memfd::new(2 * PAGE as u64).unwrap();
        let (a, b) = (Region::new(4), Region::new(4));
        a.set(1, 0, 9);
        // SAFETY: both ranges are inside regions this test owns.
        unsafe {
            map_at(a.page(1), 2 * PAGE, &pages, 0, true).unwrap();
            map_at(b.page(0), PAGE, &pages, PAGE as u64, true).unwrap();
        }
        // The object's zeros replaced what the page held.
        assert_eq!(a.get(1, 0), 0);
        a.set(2, 5, 42);
        assert_eq!(b.get(0, 5), 42);
        b.set(0, 6, 43);
        assert_eq!(a.get(2, 6), 43);
        let mut byte = [0];
        pages.read_at(&mut byte, PAGE as u64 + 6).unwrap();
        assert_eq!(byte, [43]);
    }

    #[test]
    fn mapping_private_pages_again_detaches_and_zeroes_the_range() {
        let pages = Memfd::new(PAGE as u64).unwrap();
        let region = Region::new(2);
        // SAFETY: inside a region this test owns.
        unsafe { map_at(region.page(1), PAGE, &pages, 0, true).unwrap() };
        region.set(1, 0, 1);
        // SAFETY: as above.
        unsafe { map_private_at(region.page(1), PAGE).unwrap() };
        assert_eq!(region.get(1, 0), 0);
        region.set(1, 0, 2);
        let mut byte = [0];
        pages.read_at(&mut byte, 0).unwrap();
        assert_eq!(byte, [1]);
    }

    /// Whether the host holds memory for each of the first `pages` pages at `ptr` (`mincore`).
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn resident(ptr: *mut u8, pages: usize) -> Vec<bool> {
        let host_pages = PAGE / host_page_size();
        let mut vec = vec![0u8; pages * host_pages];
        // SAFETY: a page-aligned range of a mapping the caller owns, one byte per host page.
        let ret = unsafe { libc::mincore(ptr.cast(), pages * PAGE, vec.as_mut_ptr().cast()) };
        assert_eq!(ret, 0);
        vec.chunks(host_pages)
            .map(|page| page.iter().any(|byte| byte & 1 != 0))
            .collect()
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn dropped_private_pages_read_zero_and_hold_no_memory() {
        let region = Region::new(3);
        region.set(0, 0, 1);
        region.set(1, 7, 2);
        assert_eq!(resident(region.page(0), 3), [true, true, false]);
        // SAFETY: inside a region this test owns.
        unsafe { drop_private(region.page(1), 2 * PAGE).unwrap() };
        // The written page is gone, the untouched one was not touched to drop it.
        assert_eq!(resident(region.page(0), 3), [true, false, false]);
        assert_eq!((region.get(0, 0), region.get(1, 7)), (1, 0));
        // The range stays part of the mapping, writable.
        region.set(1, 7, 3);
        assert_eq!(region.get(1, 7), 3);
        // SAFETY: as above; the call fails before it drops anything.
        let err = unsafe { drop_private(region.page(1).add(1), PAGE) }.unwrap_err();
        assert_eq!(err.raw_os_error(), Some(libc::EINVAL));
        assert_eq!(region.get(1, 7), 3);
    }

    /// Whether the kernel may store into `ptr`: `read(2)` from a pipe into it fails with
    /// `EFAULT` where it may not, where a store of the process would raise `SIGSEGV`.
    fn kernel_may_store(ptr: *mut u8) -> bool {
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for both ends.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: a one-byte buffer; `ptr` is only handed to the kernel, which checks it.
        let read = unsafe {
            assert_eq!(libc::write(fds[1], [1u8].as_ptr().cast(), 1), 1);
            libc::read(fds[0], ptr.cast(), 1)
        };
        let errno = io::Error::last_os_error().raw_os_error();
        // SAFETY: the pipe's own descriptors.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        match read {
            1 => true,
            _ => {
                assert_eq!(errno, Some(libc::EFAULT));
                false
            }
        }
    }

    #[test]
    fn a_read_only_mapping_shows_the_object_and_refuses_stores() {
        let pages = Memfd::new(PAGE as u64).unwrap();
        let (writer, reader) = (Region::new(1), Region::new(2));
        // SAFETY: inside regions this test owns; nothing stores into the read-only range.
        unsafe {
            map_at(writer.page(0), PAGE, &pages, 0, true).unwrap();
            map_at(reader.page(1), PAGE, &pages, 0, false).unwrap();
        }
        writer.set(0, 3, 7);
        assert_eq!(reader.get(1, 3), 7);
        assert!(!kernel_may_store(reader.page(1)));
        assert!(kernel_may_store(reader.page(0)));
        // Private pages in its place are writable again.
        // SAFETY: as above.
        unsafe { map_private_at(reader.page(1), PAGE).unwrap() };
        assert!(kernel_may_store(reader.page(1)));
        assert_eq!(writer.get(0, 3), 7);
    }

    #[test]
    fn a_range_past_the_object_is_refused() {
        let pages = Memfd::new(PAGE as u64).unwrap();
        let region = Region::new(2);
        // SAFETY: inside a region this test owns; the call fails before it maps anything.
        let err = unsafe { map_at(region.page(0), 2 * PAGE, &pages, 0, true) }.unwrap_err();
        assert_eq!(err.raw_os_error(), Some(libc::EINVAL));
        // SAFETY: as above.
        let err = unsafe { map_at(region.page(0), PAGE, &pages, 1, true) }.unwrap_err();
        assert_eq!(err.raw_os_error(), Some(libc::EINVAL));
    }

    #[test]
    fn an_object_without_a_descriptor_cannot_be_mapped() {
        struct Heap;
        impl SharedPages for Heap {
            fn id(&self) -> u64 {
                0
            }
            fn size(&self) -> u64 {
                PAGE as u64
            }
            fn resize(&self, _: u64) -> io::Result<()> {
                Ok(())
            }
            fn read_at(&self, _: &mut [u8], _: u64) -> io::Result<()> {
                Ok(())
            }
            fn write_at(&self, _: &[u8], _: u64) -> io::Result<()> {
                Ok(())
            }
            fn punch_hole(&self, _: u64, _: u64) -> io::Result<()> {
                Ok(())
            }
            fn allocated(&self) -> u64 {
                0
            }
        }
        let region = Region::new(1);
        // SAFETY: inside a region this test owns; the call fails before it maps anything.
        let err = unsafe { map_at(region.page(0), PAGE, &Heap, 0, true) }.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
    }
}
