// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! Low-level abstraction for allocating and managing zero-filled pages
//! of memory.

use loupe::{MemoryUsage, MemoryUsageTracker};
use std::slice;

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
        }
    }

    /// Create a new `Mmap` pointing to at least `size` bytes of page-aligned accessible memory.
    pub fn with_at_least(size: usize) -> Result<Self, String> {
        let page_size = 65536; //arbitrary 64K pages
        let rounded_size = round_up_to_page_size(size, page_size);
        Self::accessible_reserved(rounded_size, rounded_size)
    }

    /// Not implemented on FakeVM
    pub fn accessible_reserved(
        _accessible_size: usize,
        _mapping_size: usize,
    ) -> Result<Self, String> {
        panic!("Not implemented!");
    }

    /// Not implemented on FakeVM
    pub fn make_accessible(&mut self, _start: usize, _len: usize) -> Result<(), String> {
        panic!("Not implemented!");
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
}

impl Drop for Mmap {
    fn drop(&mut self) {}
}

impl MemoryUsage for Mmap {
    fn size_of_val(&self, tracker: &mut dyn MemoryUsageTracker) -> usize {
        tracker.track(self.as_ptr() as *const ());

        self.len()
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
