//! Low-level abstraction for allocating and managing zero-filled pages
//! of memory.

use errno;
use region;
use std::ptr;
use std::slice;
use std::string::String;
use winapi::um::memoryapi::{VirtualAlloc, VirtualFree};
use winapi::um::winnt::{MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS};

use super::common::round_up_to_page_size;

/// A simple struct consisting of a page-aligned pointer to page-aligned
/// and initially-zeroed memory and a length.
#[derive(Debug)]
pub struct Mmap {
    ptr: *mut u8,
    len: usize,
}

impl Mmap {
    /// Construct a new empty instance of `Mmap`.
    pub fn new() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
        }
    }

    /// Create a new `Mmap` pointing to at least `size` bytes of memory,
    /// suitably sized and aligned for memory protection.
    pub fn with_size(size: usize) -> Result<Self, String> {
        let page_size = region::page::size();

        // VirtualAlloc always rounds up to the next multiple of the page size
        let ptr = unsafe {
            VirtualAlloc(
                ptr::null_mut(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_NOACCESS,
            )
        };
        if !ptr.is_null() {
            Ok(Self {
                ptr: ptr as *mut u8,
                len: round_up_to_page_size(size, page_size),
            })
        } else {
            Err(errno::errno().to_string())
        }
    }

    /// Return the allocated memory as a slice of u8.
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Return the allocated memory as a mutable slice of u8.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Return the allocated memory as a pointer to u8.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Return the allocated memory as a mutable pointer to u8.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }

    /// Return the lengthof the allocated memory.
    pub fn len(&self) -> usize {
        self.len
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            let r = unsafe { VirtualFree(self.ptr, self.len, MEM_RELEASE) };
            assert_eq!(r, 0);
        }
    }
}
