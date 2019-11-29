use crate::error::MemoryCreationError;
use crate::error::MemoryProtectionError;
use errno;
use nix::libc;
use page_size;
use std::ops::{Bound, RangeBounds};
use std::{fs::File, os::unix::io::IntoRawFd, path::Path, ptr, slice, sync::Arc};

unsafe impl Send for Memory {}
unsafe impl Sync for Memory {}

/// Data for a sized and protected region of memory.
#[derive(Debug)]
pub struct Memory {
    ptr: *mut u8,
    size: usize,
    protection: Protect,
    fd: Option<Arc<RawFd>>,
}

impl Memory {
    /// Create a new memory from the given path value and protection.
    pub fn from_file_path<P>(path: P, protection: Protect) -> Result<Self, MemoryCreationError>
    where
        P: AsRef<Path>,
    {
        let file = File::open(path)?;

        let file_len = file.metadata()?.len();

        let raw_fd = RawFd::from_file(file);

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                file_len as usize,
                protection.to_protect_const() as i32,
                libc::MAP_PRIVATE,
                raw_fd.0,
                0,
            )
        };

        if ptr == -1 as _ {
            Err(MemoryCreationError::VirtualMemoryAllocationFailed(
                file_len as usize,
                errno::errno().to_string(),
            ))
        } else {
            Ok(Self {
                ptr: ptr as *mut u8,
                size: file_len as usize,
                protection,
                fd: Some(Arc::new(raw_fd)),
            })
        }
    }

    /// Create a new memory with the given size and protection.
    pub fn with_size_protect(size: usize, protection: Protect) -> Result<Self, String> {
        if size == 0 {
            return Ok(Self {
                ptr: ptr::null_mut(),
                size: 0,
                protection,
                fd: None,
            });
        }

        let size = round_up_to_page_size(size, page_size::get());

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                protection.to_protect_const() as i32,
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
                protection,
                fd: None,
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
                fd: None,
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
            Err(MemoryCreationError::VirtualMemoryAllocationFailed(
                size,
                errno::errno().to_string(),
            ))
        } else {
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
                protection: Protect::None,
                fd: None,
            })
        }
    }

    /// Protect this memory with the given range bounds and protection.
    pub unsafe fn protect(
        &mut self,
        range: impl RangeBounds<usize>,
        protection: Protect,
    ) -> Result<(), MemoryProtectionError> {
        let protect = protection.to_protect_const();

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
            Err(MemoryProtectionError::ProtectionFailed(
                start as usize,
                size,
                errno::errno().to_string(),
            ))
        } else {
            self.protection = protection;
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
                fd: self.fd.clone(),
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
            let success = unsafe { libc::munmap(self.ptr as _, self.size) };
            assert_eq!(success, 0, "failed to unmap memory: {}", errno::errno());
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
    /// Read/write/exec only.
    ReadWriteExec,
}

impl Protect {
    fn to_protect_const(self) -> u32 {
        match self {
            Protect::None => 0,
            Protect::Read => 1,
            Protect::ReadWrite => 1 | 2,
            Protect::ReadExec => 1 | 4,
            Protect::ReadWriteExec => 1 | 2 | 4,
        }
    }

    /// Returns true if this memory is readable.
    pub fn is_readable(self) -> bool {
        match self {
            Protect::Read | Protect::ReadWrite | Protect::ReadExec | Protect::ReadWriteExec => true,
            _ => false,
        }
    }

    /// Returns true if this memory is writable.
    pub fn is_writable(self) -> bool {
        match self {
            Protect::ReadWrite | Protect::ReadWriteExec => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
struct RawFd(i32);

impl RawFd {
    fn from_file(f: File) -> Self {
        RawFd(f.into_raw_fd())
    }
}

impl Drop for RawFd {
    fn drop(&mut self) {
        let success = unsafe { libc::close(self.0) };
        assert_eq!(
            success,
            0,
            "failed to close mmapped file descriptor: {}",
            errno::errno()
        );
    }
}

/// Round `size` up to the nearest multiple of `page_size`.
fn round_up_to_page_size(size: usize, page_size: usize) -> usize {
    assert!(page_size.is_power_of_two());
    (size + (page_size - 1)) & !(page_size - 1)
}

/// Round `size` down to the nearest multiple of `page_size`.
fn round_down_to_page_size(size: usize, page_size: usize) -> usize {
    assert!(page_size.is_power_of_two());
    size & !(page_size - 1)
}
