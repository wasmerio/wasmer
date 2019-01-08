use std::{slice, ptr};
use std::ops::Range;
use nix::libc;
use page_size;
use errno;

#[derive(Debug)]
pub struct Mmap {
    ptr: *mut u8,
    size: usize,
}

impl Mmap {
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

    pub unsafe fn protect(&mut self, range: Range<usize>, protect: Protect) -> Result<(), String> {
        let page_size = page_size::get();
        let start = self.ptr.add(round_down_to_page_size(range.start, page_size));
        let size = range.end - range.start;

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

    pub fn as_ptr(&mut self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Mmap {
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
    None = 0,
    Read = 1,
    Write = 2,
    ReadWrite = 1 | 2,
    Exec = 4,
    ReadExec = 1 | 4,
}

/// Round `size` up to the nearest multiple of `page_size`.
fn round_up_to_page_size(size: usize, page_size: usize) -> usize {
    (size + (page_size - 1)) & !(page_size - 1)
}

/// Round `size` down to the nearest multiple of `page_size`.
fn round_down_to_page_size(size: usize, page_size: usize) -> usize {
    size & !(page_size-1)
}