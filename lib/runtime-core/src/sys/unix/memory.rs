use errno;
use nix::libc;
use page_size;
use std::ops::{Bound, RangeBounds};
use std::{ptr, slice};

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

#[derive(Debug)]
pub struct Memory {
    ptr: *mut u8,
    size: usize,
}

impl Memory {
    pub fn with_size(size: usize) -> Result<Self, String> {
        if size == 0 {
            return Ok(Self {
                ptr: ptr::null_mut(),
                size: 0,
            });
        }

        let size = round_up_to_page_size(size, page_size::get());

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };

        if ptr == -1 as _ {
            Err(errno::errno().to_string())
        } else {
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
            })
        }
    }

    pub unsafe fn protect(
        &mut self,
        range: impl RangeBounds<usize>,
        protect: Protect,
    ) -> Result<(), String> {
        let protect = protect.to_protect_const();

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

        let success = libc::mprotect(start as _, size, protect as i32);
        if success == -1 {
            Err(errno::errno().to_string())
        } else {
            Ok(())
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub unsafe fn as_slice(&self) -> &[u8] {
        slice::from_raw_parts(self.ptr, self.size)
    }

    pub unsafe fn as_slice_mut(&mut self) -> &mut [u8] {
        slice::from_raw_parts_mut(self.ptr, self.size)
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            let success = unsafe { libc::munmap(self.ptr as _, self.size) };
            assert_eq!(success, 0, "failed to unmap memory: {}", errno::errno());
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Protect {
    None,
    Read,
    ReadWrite,
    ReadExec,
}

impl Protect {
    fn to_protect_const(self) -> u32 {
        match self {
            Protect::None => 0,
            Protect::Read => 1,
            Protect::ReadWrite => 1 | 2,
            Protect::ReadExec => 1 | 4,
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
