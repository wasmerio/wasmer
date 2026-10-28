//! The host's anonymous shared memory file: `memfd_create` on Linux (`mm/memfd.c`, a file on
//! the internal shmem mount), and on macOS a POSIX shared memory object unlinked as soon as it
//! is open, so that like a memfd only its descriptors reach it.

use std::{
    fs::File,
    io,
    os::{
        fd::{AsFd, BorrowedFd, FromRawFd, OwnedFd},
        unix::fs::{FileExt, MetadataExt},
    },
};

use crate::{SharedPages, new_id};

/// [`SharedPages`] in an anonymous host file, which every mapping of it shares.
///
/// On macOS the size can be set only once, while it is 0: [`SharedPages::resize`] fails after.
pub struct Memfd {
    file: File,
    id: u64,
}

impl Memfd {
    /// A new object of `size` bytes, all zero, which takes host memory only as it is written.
    pub fn new(size: u64) -> io::Result<Self> {
        let memfd = Self {
            file: File::from(create()?),
            id: new_id(),
        };
        memfd.file.set_len(size)?;
        Ok(memfd)
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn create() -> io::Result<OwnedFd> {
    // SAFETY: a NUL-terminated name and flags `memfd_create` knows.
    let fd = unsafe { libc::memfd_create(c"wasixv2-shm".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a descriptor just opened, owned by nothing else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(target_os = "macos")]
fn create() -> io::Result<OwnedFd> {
    // A name nobody else uses (`PSHMNAMLEN` is 31), gone again once the object is open.
    let name = format!("/wasixv2.{}.{}", std::process::id(), new_id());
    let name = std::ffi::CString::new(name).map_err(io::Error::other)?;
    let flags = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL;
    let mode: libc::c_uint = 0o600;
    // SAFETY: a NUL-terminated name; the mode is `shm_open`'s variadic argument.
    let fd = unsafe { libc::shm_open(name.as_ptr(), flags, mode) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a descriptor just opened, owned by nothing else.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: the name created above.
    unsafe { libc::shm_unlink(name.as_ptr()) };
    Ok(fd)
}

impl SharedPages for Memfd {
    fn id(&self) -> u64 {
        self.id
    }

    fn size(&self) -> u64 {
        self.file.metadata().map_or(0, |meta| meta.len())
    }

    fn resize(&self, size: u64) -> io::Result<()> {
        self.file.set_len(size)
    }

    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let mut done = 0;
        while done < buf.len() {
            match self.file.read_at(&mut buf[done..], offset + done as u64)? {
                0 => break,
                n => done += n,
            }
        }
        buf[done..].fill(0);
        Ok(())
    }

    fn write_at(&self, data: &[u8], offset: u64) -> io::Result<()> {
        self.file.write_all_at(data, offset)
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn punch_hole(&self, offset: u64, len: u64) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        let mode = libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE;
        let (offset, len) = (to_off(offset)?, to_off(len)?);
        // SAFETY: the object's own descriptor; the range is checked by the host.
        let ret = unsafe { libc::fallocate(self.file.as_raw_fd(), mode, offset, len) };
        match ret {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// macOS has no hole punching for shared memory: the bytes are overwritten with zeros.
    #[cfg(target_os = "macos")]
    fn punch_hole(&self, offset: u64, len: u64) -> io::Result<()> {
        const ZEROS: [u8; 1 << 16] = [0; 1 << 16];
        let end = offset.saturating_add(len).min(self.size());
        let mut at = offset;
        while at < end {
            let n = (end - at).min(ZEROS.len() as u64);
            self.file.write_all_at(&ZEROS[..n as usize], at)?;
            at += n;
        }
        Ok(())
    }

    fn allocated(&self) -> u64 {
        // `st_blocks` counts 512-byte units whatever the file system's block size.
        self.file.metadata().map_or(0, |meta| meta.blocks() * 512)
    }

    fn fd(&self) -> Option<BorrowedFd<'_>> {
        Some(self.file.as_fd())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn to_off(value: u64) -> io::Result<libc::off_t> {
    libc::off_t::try_from(value).map_err(|_| io::Error::from_raw_os_error(libc::EFBIG))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: u64 = 65536;

    #[test]
    fn a_new_object_reads_zero_and_keeps_what_is_written() {
        let pages = Memfd::new(2 * PAGE).unwrap();
        assert_eq!(pages.size(), 2 * PAGE);
        pages.write_at(b"shared", PAGE - 3).unwrap();
        let mut buf = [1; 10];
        pages.read_at(&mut buf, PAGE - 5).unwrap();
        assert_eq!(&buf, b"\0\0shared\0\0");
    }

    #[test]
    fn bytes_past_the_end_read_zero() {
        let pages = Memfd::new(PAGE).unwrap();
        pages.write_at(b"xy", PAGE - 2).unwrap();
        let mut buf = [1; 4];
        pages.read_at(&mut buf, PAGE - 2).unwrap();
        assert_eq!(&buf, b"xy\0\0");
    }

    #[test]
    fn a_punched_hole_reads_zero_and_keeps_the_size() {
        let pages = Memfd::new(2 * PAGE).unwrap();
        pages.write_at(&[7; 2 * PAGE as usize], 0).unwrap();
        pages.punch_hole(0, PAGE).unwrap();
        let mut buf = [1; 2];
        pages.read_at(&mut buf, PAGE - 1).unwrap();
        assert_eq!(buf, [0, 7]);
        assert_eq!(pages.size(), 2 * PAGE);
    }

    #[test]
    fn host_memory_is_taken_as_pages_are_written() {
        let pages = Memfd::new(16 * PAGE).unwrap();
        assert_eq!(pages.allocated(), 0);
        pages.write_at(&[1; PAGE as usize], 0).unwrap();
        assert!(pages.allocated() >= PAGE);
        assert!(pages.allocated() < 16 * PAGE);
    }

    #[test]
    fn every_object_has_an_id_of_its_own() {
        let (a, b) = (Memfd::new(PAGE).unwrap(), Memfd::new(PAGE).unwrap());
        assert_ne!(a.id(), b.id());
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn resize_frees_the_tail_and_grows_with_zeros() {
        let pages = Memfd::new(2 * PAGE).unwrap();
        pages.write_at(&[7; 2 * PAGE as usize], 0).unwrap();
        pages.resize(PAGE).unwrap();
        pages.resize(2 * PAGE).unwrap();
        let mut buf = [1; 2];
        pages.read_at(&mut buf, PAGE - 1).unwrap();
        assert_eq!(buf, [7, 0]);
    }
}
