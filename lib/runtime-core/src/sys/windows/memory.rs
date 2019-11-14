use crate::error::MemoryCreationError;
use crate::error::MemoryProtectionError;
use page_size;
use std::ops::{Bound, RangeBounds};
use std::{ptr, slice};
use winapi::um::memoryapi::{VirtualAlloc, VirtualFree};
use winapi::um::winnt::{
    MEM_COMMIT, MEM_DECOMMIT, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_NOACCESS, PAGE_READONLY,
    PAGE_READWRITE,
};

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

/// Data for a sized and protected region of memory.
#[derive(Debug)]
pub struct Memory {
    ptr: *mut u8,
    size: usize,
    protection: Protect,
}

impl Memory {
    /// Create a new memory from the given path value and protection.
    pub fn with_size_protect(size: usize, protection: Protect) -> Result<Self, String> {
        if size == 0 {
            return Ok(Self {
                ptr: ptr::null_mut(),
                size: 0,
                protection,
            });
        }

        let size = round_up_to_page_size(size, page_size::get());

        let protect = protection.to_protect_const();

        let flags = if protection == Protect::None {
            MEM_RESERVE
        } else {
            MEM_RESERVE | MEM_COMMIT
        };

        let ptr = unsafe { VirtualAlloc(ptr::null_mut(), size, flags, protect) };

        if ptr.is_null() {
            Err("unable to allocate memory".to_string())
        } else {
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
                protection,
            })
        }
    }

    /// Create a new memory with the given size.
    pub fn with_size(size: usize) -> Result<Self, MemoryCreationError> {
        if size == 0 {
            return Ok(Self {
                ptr: ptr::null_mut(),
                size: 0,
                protection: Protect::None,
            });
        }

        let size = round_up_to_page_size(size, page_size::get());

        let ptr = unsafe { VirtualAlloc(ptr::null_mut(), size, MEM_RESERVE, PAGE_NOACCESS) };

        if ptr.is_null() {
            Err(MemoryCreationError::VirtualMemoryAllocationFailed(
                size,
                "unable to allocate memory".to_string(),
            ))
        } else {
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
                protection: Protect::None,
            })
        }
    }

    /// Protect this memory with the given range bounds and protection.
    pub unsafe fn protect(
        &mut self,
        range: impl RangeBounds<usize>,
        protect: Protect,
    ) -> Result<(), MemoryProtectionError> {
        let protect_const = protect.to_protect_const();

        let range_start = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => *start,
            Bound::Unbounded => 0,
        };

        let range_end = match range.end_bound() {
            Bound::Included(end) => *end,
            Bound::Excluded(end) => *end,
            Bound::Unbounded => self.size(),
        };

        let page_size = page_size::get();
        let start = self
            .ptr
            .add(round_down_to_page_size(range_start, page_size));
        let size = round_up_to_page_size(range_end - range_start, page_size);
        assert!(size <= self.size);

        // Commit the virtual memory.
        let ptr = VirtualAlloc(start as _, size, MEM_COMMIT, protect_const);

        if ptr.is_null() {
            Err(MemoryProtectionError::ProtectionFailed(
                start as usize,
                size,
                "unable to protect memory".to_string(),
            ))
        } else {
            self.protection = protect;
            Ok(())
        }
    }

    /// Split this memory into multiple memories by the given offset.
    pub fn split_at(mut self, offset: usize) -> (Memory, Memory) {
        let page_size = page_size::get();
        if offset % page_size == 0 {
            let second_ptr = unsafe { self.ptr.add(offset) };
            let second_size = self.size - offset;

            self.size = offset;

            let second = Memory {
                ptr: second_ptr,
                size: second_size,
                protection: self.protection,
            };

            (self, second)
        } else {
            panic!("offset must be multiple of page size: {}", offset)
        }
    }

    /// Gets the size of this memory.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Gets a slice for this memory.
    pub unsafe fn as_slice(&self) -> &[u8] {
        slice::from_raw_parts(self.ptr, self.size)
    }

    /// Gets a mutable slice for this memory.
    pub unsafe fn as_slice_mut(&mut self) -> &mut [u8] {
        slice::from_raw_parts_mut(self.ptr, self.size)
    }

    /// Gets the protect kind of this memory.
    pub fn protection(&self) -> Protect {
        self.protection
    }

    /// Gets mutable pointer to the memory.
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            let success = unsafe { VirtualFree(self.ptr as _, self.size, MEM_DECOMMIT) };
            // If the function succeeds, the return value is nonzero.
            assert_eq!(success, 1, "failed to unmap memory: {}", errno::errno());
        }
    }
}

impl Clone for Memory {
    fn clone(&self) -> Self {
        let temp_protection = if self.protection.is_writable() {
            self.protection
        } else {
            Protect::ReadWrite
        };

        let mut new = Memory::with_size_protect(self.size, temp_protection).unwrap();
        unsafe {
            new.as_slice_mut().copy_from_slice(self.as_slice());

            if temp_protection != self.protection {
                new.protect(.., self.protection).unwrap();
            }
        }

        new
    }
}

/// Kinds of memory protection.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Protect {
    /// Read/write/exec allowed.
    None,
    /// Read only.
    Read,
    /// Read/write only.
    ReadWrite,
    /// Read/exec only.
    ReadExec,
}

impl Protect {
    fn to_protect_const(self) -> u32 {
        match self {
            Protect::None => PAGE_NOACCESS,
            Protect::Read => PAGE_READONLY,
            Protect::ReadWrite => PAGE_READWRITE,
            Protect::ReadExec => PAGE_EXECUTE_READ,
        }
    }

    /// Returns true if this memory is readable.
    pub fn is_readable(self) -> bool {
        match self {
            Protect::Read | Protect::ReadWrite | Protect::ReadExec => true,
            _ => false,
        }
    }

    /// Returns true if this memory is writable.
    pub fn is_writable(self) -> bool {
        match self {
            Protect::ReadWrite => true,
            _ => false,
        }
    }
}

/// Round `size` up to the nearest multiple of `page_size`.
fn round_up_to_page_size(size: usize, page_size: usize) -> usize {
    (size + (page_size - 1)) & !(page_size - 1)
}

/// Round `size` down to the nearest multiple of `page_size`.
fn round_down_to_page_size(size: usize, page_size: usize) -> usize {
    size & !(page_size - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone() {
        // these should work
        let _ = Memory::with_size_protect(200_000, Protect::Read)
            .unwrap()
            .clone();
        let _ = Memory::with_size_protect(200_000, Protect::ReadWrite)
            .unwrap()
            .clone();
        let _ = Memory::with_size_protect(200_000, Protect::ReadExec)
            .unwrap()
            .clone();

        // this would cause segmentation fault as uncommited memory with no access
        //let _ = Memory::with_size_protect(200_000, Protect::None).unwrap().clone();
    }
}
