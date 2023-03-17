// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

use std::{
    io::{self, Read, Write},
    ptr, slice,
};

// /// Round `size` up to the nearest multiple of `page_size`.
// fn round_up_to_page_size(size: usize, page_size: usize) -> usize {
//     (size + (page_size - 1)) & !(page_size - 1)
// }

/// A simple struct consisting of a page-aligned pointer to page-aligned
/// and initially-zeroed memory and a length.
#[derive(Debug)]
pub struct FdMmap {
    // Note that this is stored as a `usize` instead of a `*const` or `*mut`
    // pointer to allow this structure to be natively `Send` and `Sync` without
    // `unsafe impl`. This type is sendable across threads and shareable since
    // the coordination all happens at the OS layer.
    ptr: usize,
    len: usize,
    // Backing file that will be closed when the memory mapping goes out of scope
    fd: FdGuard,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FdGuard(pub i32);

impl Default for FdGuard {
    fn default() -> Self {
        Self(-1)
    }
}

impl Clone for FdGuard {
    fn clone(&self) -> Self {
        unsafe { Self(libc::dup(self.0)) }
    }
}

impl Drop for FdGuard {
    fn drop(&mut self) {
        if self.0 >= 0 {
            unsafe {
                libc::close(self.0);
            }
            self.0 = -1;
        }
    }
}

impl FdMmap {
    /// Construct a new empty instance of `Mmap`.
    pub fn new() -> Self {
        // Rust's slices require non-null pointers, even when empty. `Vec`
        // contains code to create a non-null dangling pointer value when
        // constructed empty, so we reuse that here.
        let empty = Vec::<u8>::new();
        Self {
            ptr: empty.as_ptr() as usize,
            len: 0,
            fd: FdGuard::default(),
        }
    }

    // /// Create a new `Mmap` pointing to at least `size` bytes of page-aligned accessible memory.
    // pub fn with_at_least(size: usize) -> Result<Self, String> {
    //     let page_size = region::page::size();
    //     let rounded_size = round_up_to_page_size(size, page_size);
    //     Self::accessible_reserved(rounded_size, rounded_size)
    // }

    /// Create a new `Mmap` pointing to `accessible_size` bytes of page-aligned accessible memory,
    /// within a reserved mapping of `mapping_size` bytes. `accessible_size` and `mapping_size`
    /// must be native page-size multiples.
    pub fn accessible_reserved(
        accessible_size: usize,
        mapping_size: usize,
    ) -> Result<Self, String> {
        let page_size = region::page::size();
        assert!(accessible_size <= mapping_size);
        assert_eq!(mapping_size & (page_size - 1), 0);
        assert_eq!(accessible_size & (page_size - 1), 0);

        // Mmap may return EINVAL if the size is zero, so just
        // special-case that.
        if mapping_size == 0 {
            return Ok(Self::new());
        }

        // Open a temporary file (which is used for swapping)
        let fd = unsafe {
            let file = libc::tmpfile();
            if file.is_null() {
                return Err(format!(
                    "failed to create temporary file - {}",
                    io::Error::last_os_error()
                ));
            }
            FdGuard(libc::fileno(file))
        };

        // First we initialize it with zeros
        unsafe {
            if libc::ftruncate(fd.0, mapping_size as libc::off_t) < 0 {
                return Err("could not truncate tmpfile".to_string());
            }
        }

        // Compute the flags
        let flags = libc::MAP_FILE | libc::MAP_SHARED;

        Ok(if accessible_size == mapping_size {
            // Allocate a single read-write region at once.
            let ptr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    mapping_size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    flags,
                    fd.0,
                    0,
                )
            };
            if ptr as isize == -1_isize {
                return Err(io::Error::last_os_error().to_string());
            }

            Self {
                ptr: ptr as usize,
                len: mapping_size,
                fd,
            }
        } else {
            // Reserve the mapping size.
            let ptr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    mapping_size,
                    libc::PROT_NONE,
                    flags,
                    fd.0,
                    0,
                )
            };
            if ptr as isize == -1_isize {
                return Err(io::Error::last_os_error().to_string());
            }

            let mut result = Self {
                ptr: ptr as usize,
                len: mapping_size,
                fd,
            };

            if accessible_size != 0 {
                // Commit the accessible size.
                result.make_accessible(0, accessible_size)?;
            }

            result
        })
    }

    /// Make the memory starting at `start` and extending for `len` bytes accessible.
    /// `start` and `len` must be native page-size multiples and describe a range within
    /// `self`'s reserved memory.
    pub fn make_accessible(&mut self, start: usize, len: usize) -> Result<(), String> {
        let page_size = region::page::size();
        assert_eq!(start & (page_size - 1), 0);
        assert_eq!(len & (page_size - 1), 0);
        assert!(len < self.len);
        assert!(start < self.len - len);

        // Commit the accessible size.
        let ptr = self.ptr as *const u8;
        unsafe { region::protect(ptr.add(start), len, region::Protection::READ_WRITE) }
            .map_err(|e| e.to_string())
    }

    /// Make the entire memory inaccessible to both reads and writes.
    pub fn make_all_inaccessible(&self) -> Result<(), String> {
        self.make_inaccessible(0, self.len)
    }

    /// Make the memory starting at `start` and extending for `len` bytes inaccessible
    /// to both reads and writes.
    /// `start` and `len` must be native page-size multiples and describe a range within
    /// `self`'s reserved memory.
    pub fn make_inaccessible(&self, start: usize, len: usize) -> Result<(), String> {
        let page_size = region::page::size();
        assert_eq!(start & (page_size - 1), 0);
        assert_eq!(len & (page_size - 1), 0);
        assert!(len <= self.len);
        assert!(start <= self.len - len);

        // Commit the accessible size.
        let ptr = self.ptr as *const u8;
        unsafe { region::protect(ptr.add(start), len, region::Protection::NONE) }
            .map_err(|e| e.to_string())
    }

    /// Return the allocated memory as a slice of u8.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }

    /// Return the allocated memory as a mutable slice of u8.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr as *mut u8, self.len) }
    }

    // /// Return the allocated memory as a pointer to u8.
    // pub fn as_ptr(&self) -> *const u8 {
    //     self.ptr as *const u8
    // }

    /// Return the allocated memory as a mutable pointer to u8.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr as *mut u8
    }

    /// Return the length of the allocated memory.
    pub fn len(&self) -> usize {
        self.len
    }

    // /// Return whether any memory has been allocated.
    // pub fn is_empty(&self) -> bool {
    //     self.len() == 0
    // }

    /// Copies the memory to a new swap file (using copy-on-write if available)
    pub fn duplicate(&mut self, hint_used: Option<usize>) -> Result<Self, String> {
        // Empty memory is an edge case

        use std::os::unix::prelude::FromRawFd;
        if self.len == 0 {
            return Ok(Self::new());
        }

        // First we sync all the data to the backing file
        unsafe {
            libc::fsync(self.fd.0);
        }

        // Open a new temporary file (which is used for swapping for the forked memory)
        let fd = unsafe {
            let file = libc::tmpfile();
            if file.is_null() {
                return Err(format!(
                    "failed to create temporary file - {}",
                    io::Error::last_os_error()
                ));
            }
            FdGuard(libc::fileno(file))
        };

        // Attempt to do a shallow copy (needs a backing file system that supports it)
        unsafe {
            if libc::ioctl(fd.0, 0x94, 9, self.fd.0) != 0
            // FICLONE
            {
                #[cfg(feature = "tracing")]
                trace!("memory copy started");

                // Determine host much to copy
                let len = match hint_used {
                    Some(a) => a,
                    None => self.len,
                };

                // The shallow copy failed so we have to do it the hard way

                let mut source = std::fs::File::from_raw_fd(self.fd.0);
                let mut out = std::fs::File::from_raw_fd(fd.0);
                copy_file_range(&mut source, 0, &mut out, 0, len)
                    .map_err(|err| format!("Could not copy memory: {err}"))?;

                #[cfg(feature = "tracing")]
                trace!("memory copy finished (size={})", len);
            }
        }

        // Compute the flags
        let flags = libc::MAP_FILE | libc::MAP_SHARED;

        // Allocate a single read-write region at once.
        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                self.len,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                fd.0,
                0,
            )
        };
        if ptr as isize == -1_isize {
            return Err(io::Error::last_os_error().to_string());
        }

        Ok(Self {
            ptr: ptr as usize,
            len: self.len,
            fd,
        })
    }
}

impl Drop for FdMmap {
    fn drop(&mut self) {
        if self.len != 0 {
            let r = unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
            assert_eq!(r, 0, "munmap failed: {}", io::Error::last_os_error());
        }
    }
}

/// Copy a range of a file to another file.
// We could also use libc::copy_file_range on some systems, but it's
// hard to do this because it is not available on many libc implementations.
// (not on Mac OS, musl, ...)
#[cfg(target_family = "unix")]
fn copy_file_range(
    source: &mut std::fs::File,
    source_offset: u64,
    out: &mut std::fs::File,
    out_offset: u64,
    len: usize,
) -> Result<(), std::io::Error> {
    use std::io::{Seek, SeekFrom};

    let source_original_pos = source.stream_position()?;
    source.seek(SeekFrom::Start(source_offset))?;

    // TODO: don't cast with as

    let out_original_pos = out.stream_position()?;
    out.seek(SeekFrom::Start(out_offset))?;

    // TODO: don't do this horrible "triple buffering" below".
    // let mut reader = std::io::BufReader::new(source);

    // TODO: larger buffer?
    let mut buffer = vec![0u8; 4096];

    let mut to_read = len;
    while to_read > 0 {
        let chunk_size = std::cmp::min(to_read, buffer.len());
        let read = source.read(&mut buffer[0..chunk_size])?;
        out.write_all(&buffer[0..read])?;
        to_read -= read;
    }

    // Need to read the last chunk.
    out.flush()?;

    // Restore files to original position.
    source.seek(SeekFrom::Start(source_original_pos))?;
    out.flush()?;
    out.sync_data()?;
    out.seek(SeekFrom::Start(out_original_pos))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_round_up_to_page_size() {
    //     assert_eq!(round_up_to_page_size(0, 4096), 0);
    //     assert_eq!(round_up_to_page_size(1, 4096), 4096);
    //     assert_eq!(round_up_to_page_size(4096, 4096), 4096);
    //     assert_eq!(round_up_to_page_size(4097, 4096), 8192);
    // }

    #[cfg(target_family = "unix")]
    #[test]
    fn test_copy_file_range() -> Result<(), std::io::Error> {
        // I know tempfile:: exists, but this doesn't bring in an extra
        // dependency.

        use std::{fs::OpenOptions, io::Seek};

        let dir = std::env::temp_dir().join("wasmer/copy_file_range");
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).unwrap()
        }
        std::fs::create_dir_all(&dir).unwrap();

        let pa = dir.join("a");
        let pb = dir.join("b");

        let data: Vec<u8> = (0..100).collect();
        let mut a = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&pa)
            .unwrap();
        a.write_all(&data).unwrap();

        let datb: Vec<u8> = (100..200).collect();
        let mut b = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&pb)
            .unwrap();
        b.write_all(&datb).unwrap();

        a.seek(io::SeekFrom::Start(30)).unwrap();
        b.seek(io::SeekFrom::Start(99)).unwrap();
        copy_file_range(&mut a, 10, &mut b, 40, 15).unwrap();

        assert_eq!(a.stream_position().unwrap(), 30);
        assert_eq!(b.stream_position().unwrap(), 99);

        b.seek(io::SeekFrom::Start(0)).unwrap();
        let mut out = Vec::new();
        let len = b.read_to_end(&mut out).unwrap();
        assert_eq!(len, 100);
        assert_eq!(out[0..40], datb[0..40]);
        assert_eq!(out[40..55], data[10..25]);
        assert_eq!(out[55..100], datb[55..100]);

        // TODO: needs more variant tests, but this is enough for now.

        Ok(())
    }
}
