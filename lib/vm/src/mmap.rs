// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! Low-level abstraction for allocating and managing zero-filled pages
//! of memory.

use more_asserts::assert_le;
use more_asserts::assert_lt;
use std::io;
use std::ptr;
use std::slice;
#[cfg(feature="tracing")]
use tracing::trace;

/// Round `size` up to the nearest multiple of `page_size`.
fn round_up_to_page_size(size: usize, page_size: usize) -> usize {
    (size + (page_size - 1)) & !(page_size - 1)
}

/// A simple struct consisting of a page-aligned pointer to page-aligned
/// and initially-zeroed memory and a length.
#[derive(Debug)]
pub struct Mmap {
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

impl Default
for FdGuard
{
    fn default() -> Self {
        Self(-1)
    }
}

impl Clone
for FdGuard
{
    fn clone(&self) -> Self {
        unsafe {
            FdGuard(libc::dup(self.0))
        }
    }
}

impl Drop
for FdGuard {
    fn drop(&mut self) {
        if self.0 >= 0 {
            unsafe { libc::close(self.0); }
            self.0 = -1;
        }
    }
}

impl Mmap {
    /// Construct a new empty instance of `Mmap`.
    pub fn new() -> Self {
        // Rust's slices require non-null pointers, even when empty. `Vec`
        // contains code to create a non-null dangling pointer value when
        // constructed empty, so we reuse that here.
        let empty = Vec::<u8>::new();
        Self {
            ptr: empty.as_ptr() as usize,
            len: 0,
            fd: FdGuard(-1),
        }
    }

    /// Create a new `Mmap` pointing to at least `size` bytes of page-aligned accessible memory.
    pub fn with_at_least(size: usize) -> Result<Self, String> {
        let page_size = region::page::size();
        let rounded_size = round_up_to_page_size(size, page_size);
        Self::accessible_reserved(rounded_size, rounded_size)
    }

    /// Create a new `Mmap` pointing to `accessible_size` bytes of page-aligned accessible memory,
    /// within a reserved mapping of `mapping_size` bytes. `accessible_size` and `mapping_size`
    /// must be native page-size multiples.
    #[cfg(not(target_os = "windows"))]
    pub fn accessible_reserved(
        accessible_size: usize,
        mapping_size: usize,
    ) -> Result<Self, String> {
        
        let page_size = region::page::size();
        assert_le!(accessible_size, mapping_size);
        assert_eq!(mapping_size & (page_size - 1), 0);
        assert_eq!(accessible_size & (page_size - 1), 0);

        // Mmap may return EINVAL if the size is zero, so just
        // special-case that.
        if mapping_size == 0 {
            return Ok(Self::new());
        }

        // Open a temporary file (which is used for swapping)
        let fd = unsafe {
            let file = if mapping_size > (u32::MAX as usize) {
                libc::tmpfile64()
            } else {
                libc::tmpfile()
            };
            if file == ptr::null_mut() {
                return Err(format!("failed to create temporary file - {}", io::Error::last_os_error()));
            }
            FdGuard(libc::fileno(file))
        };

        // First we initialize it with zeros
        if mapping_size > (u32::MAX as usize) {
            unsafe { libc::ftruncate64(fd.0, mapping_size as i64); }
        } else {
            unsafe { libc::ftruncate(fd.0, mapping_size as i64); }
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

    /// Create a new `Mmap` pointing to `accessible_size` bytes of page-aligned accessible memory,
    /// within a reserved mapping of `mapping_size` bytes. `accessible_size` and `mapping_size`
    /// must be native page-size multiples.
    #[cfg(target_os = "windows")]
    pub fn accessible_reserved(
        accessible_size: usize,
        mapping_size: usize,
    ) -> Result<Self, String> {
        use winapi::um::memoryapi::VirtualAlloc;
        use winapi::um::winnt::{MEM_COMMIT, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE};

        let page_size = region::page::size();
        assert_le!(accessible_size, mapping_size);
        assert_eq!(mapping_size & (page_size - 1), 0);
        assert_eq!(accessible_size & (page_size - 1), 0);

        // VirtualAlloc may return ERROR_INVALID_PARAMETER if the size is zero,
        // so just special-case that.
        if mapping_size == 0 {
            return Ok(Self::new());
        }

        Ok(if accessible_size == mapping_size {
            // Allocate a single read-write region at once.
            let ptr = unsafe {
                VirtualAlloc(
                    ptr::null_mut(),
                    mapping_size,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_READWRITE,
                )
            };
            if ptr.is_null() {
                return Err(io::Error::last_os_error().to_string());
            }

            Self {
                ptr: ptr as usize,
                len: mapping_size,
            }
        } else {
            // Reserve the mapping size.
            let ptr =
                unsafe { VirtualAlloc(ptr::null_mut(), mapping_size, MEM_RESERVE, PAGE_NOACCESS) };
            if ptr.is_null() {
                return Err(io::Error::last_os_error().to_string());
            }

            let mut result = Self {
                ptr: ptr as usize,
                len: mapping_size,
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
    #[cfg(not(target_os = "windows"))]
    pub fn make_accessible(&mut self, start: usize, len: usize) -> Result<(), String> {
        let page_size = region::page::size();
        assert_eq!(start & (page_size - 1), 0);
        assert_eq!(len & (page_size - 1), 0);
        assert_lt!(len, self.len);
        assert_lt!(start, self.len - len);

        // Commit the accessible size.
        let ptr = self.ptr as *const u8;
        unsafe { region::protect(ptr.add(start), len, region::Protection::READ_WRITE) }
            .map_err(|e| e.to_string())
    }

    /// Make the memory starting at `start` and extending for `len` bytes accessible.
    /// `start` and `len` must be native page-size multiples and describe a range within
    /// `self`'s reserved memory.
    #[cfg(target_os = "windows")]
    pub fn make_accessible(&mut self, start: usize, len: usize) -> Result<(), String> {
        use winapi::ctypes::c_void;
        use winapi::um::memoryapi::VirtualAlloc;
        use winapi::um::winnt::{MEM_COMMIT, PAGE_READWRITE};
        let page_size = region::page::size();
        assert_eq!(start & (page_size - 1), 0);
        assert_eq!(len & (page_size - 1), 0);
        assert_lt!(len, self.len);
        assert_lt!(start, self.len - len);

        // Commit the accessible size.
        let ptr = self.ptr as *const u8;
        if unsafe {
            VirtualAlloc(
                ptr.add(start) as *mut c_void,
                len,
                MEM_COMMIT,
                PAGE_READWRITE,
            )
        }
        .is_null()
        {
            return Err(io::Error::last_os_error().to_string());
        }

        Ok(())
    }

    /// Return the allocated memory as a slice of u8.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }

    /// Return the allocated memory as a mutable slice of u8.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr as *mut u8, self.len) }
    }

    /// Return the allocated memory as a pointer to u8.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr as *const u8
    }

    /// Return the allocated memory as a mutable pointer to u8.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr as *mut u8
    }

    /// Return the length of the allocated memory.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Return whether any memory has been allocated.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copies the memory to a new swap file (using copy-on-write if available)
    #[cfg(not(target_os = "windows"))]
    pub fn fork(&mut self, hint_used: Option<usize>) -> Result<Self, String>
    {
        // Empty memory is an edge case
        if self.len == 0 {
            return Ok(Self::new());
        }

        // First we sync all the data to the backing file
        unsafe { libc::fdatasync(self.fd.0); }

        // Open a new temporary file (which is used for swapping for the forked memory)
        let fd = unsafe {
            let file = if self.len > (u32::MAX as usize) {
                libc::tmpfile64()
            } else {
                libc::tmpfile()
            };
            if file == ptr::null_mut() {
                return Err(format!("failed to create temporary file - {}", io::Error::last_os_error()));
            }
            FdGuard(libc::fileno(file))
        };

        // Attempt to do a shallow copy (needs a backing file system that supports it)
        unsafe {
            if libc::ioctl(fd.0, 0x94, 9, self.fd.0) != 0 // FICLONE
            {
                #[cfg(feature="tracing")]
                trace!("memory copy started");

                // Determine host much to copy
                let len = match hint_used {
                    Some(a) => a,
                    None => self.len
                };

                // The shallow copy failed so we have to do it the hard way
                let mut off_in: libc::off64_t = 0;
                let mut off_out: libc::off64_t = 0;
                let ret = libc::copy_file_range(self.fd.0, &mut off_in, fd.0, &mut off_out, len, 0);
                if ret < 0 {
                    return Err(format!("failed to copy temporary file data - {}", io::Error::last_os_error()));
                }

                #[cfg(feature="tracing")]
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

        Ok(
            Self {
                ptr: ptr as usize,
                len: self.len,
                fd,
            }
        )
    }

    /// Copies the memory to a new swap file (using copy-on-write if available)
    #[cfg(target_os = "windows")]
    pub fn fork(&mut self, hint_used: Option<usize>) -> Result<Self, String>
    {
        // Create a new memory which we will copy to
        let new_mmap = Self::with_at_least(self.len)?;

        #[cfg(feature="tracing")]
        trace!("memory copy started");

        // Determine host much to copy
        let len = match hint_used {
            Some(a) => a,
            None => self.len
        };

        // Copy the data to the new memory
        let dst = new_mmap.ptr as *mut u8;
        let src = self.ptr as *const u8;
        unsafe {
            std::ptr::copy_nonoverlapping(src, dst, len);
        }

        #[cfg(feature="tracing")]
        trace!("memory copy finished (size={})", len);
        Ok(
            new_mmap
        )
    }
}

impl Drop for Mmap {
    #[cfg(not(target_os = "windows"))]
    fn drop(&mut self) {
        if self.len != 0 {
            let r = unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
            assert_eq!(r, 0, "munmap failed: {}", io::Error::last_os_error());
        }
    }

    #[cfg(target_os = "windows")]
    fn drop(&mut self) {
        if self.len != 0 {
            use winapi::ctypes::c_void;
            use winapi::um::memoryapi::VirtualFree;
            use winapi::um::winnt::MEM_RELEASE;
            let r = unsafe { VirtualFree(self.ptr as *mut c_void, 0, MEM_RELEASE) };
            assert_ne!(r, 0);
        }
    }
}

fn _assert() {
    fn _assert_send_sync<T: Send + Sync>() {}
    _assert_send_sync::<Mmap>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_up_to_page_size() {
        assert_eq!(round_up_to_page_size(0, 4096), 0);
        assert_eq!(round_up_to_page_size(1, 4096), 4096);
        assert_eq!(round_up_to_page_size(4096, 4096), 4096);
        assert_eq!(round_up_to_page_size(4097, 4096), 8192);
    }
}
