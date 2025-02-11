// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Low-level abstraction for allocating and managing zero-filled pages
//! of memory.

use more_asserts::assert_le;
use std::io;
use std::ptr;
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
    total_size: usize,
    accessible_size: usize,
    sync_on_drop: bool,
}

/// The type of mmap to create
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MmapType {
    /// The memory is private to the process and not shared with other processes.
    Private,
    /// The memory is shared with other processes. This is only supported on Unix.
    /// When the memory is flushed it will update the file data.
    Shared,
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
            total_size: 0,
            accessible_size: 0,
            sync_on_drop: false,
        }
    }

    /// Create a new `Mmap` pointing to at least `size` bytes of page-aligned accessible memory.
    pub fn with_at_least(size: usize) -> Result<Self, String> {
        let page_size = region::page::size();
        let rounded_size = round_up_to_page_size(size, page_size);
        Self::accessible_reserved(rounded_size, rounded_size, None, MmapType::Private)
    }

    /// Create a new `Mmap` pointing to `accessible_size` bytes of page-aligned accessible memory,
    /// within a reserved mapping of `mapping_size` bytes. `accessible_size` and `mapping_size`
    /// must be native page-size multiples.
    #[cfg(not(target_os = "windows"))]
    pub fn accessible_reserved(
        mut accessible_size: usize,
        mapping_size: usize,
        mut backing_file: Option<std::path::PathBuf>,
        memory_type: MmapType,
    ) -> Result<Self, String> {
        use std::os::fd::IntoRawFd;

        let page_size = region::page::size();
        assert_le!(accessible_size, mapping_size);
        assert_eq!(mapping_size & (page_size - 1), 0);
        assert_eq!(accessible_size & (page_size - 1), 0);

        // Mmap may return EINVAL if the size is zero, so just
        // special-case that.
        if mapping_size == 0 {
            return Ok(Self::new());
        }

        // If there is a backing file, resize the file so that its at least
        // `mapping_size` bytes.
        let mut memory_fd = -1;
        if let Some(backing_file_path) = &mut backing_file {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&backing_file_path)
                .map_err(|e| e.to_string())?;

            let mut backing_file_accessible = backing_file_path.clone();
            backing_file_accessible.set_extension("accessible");

            let len = file.metadata().map_err(|e| e.to_string())?.len() as usize;
            if len < mapping_size {
                std::fs::write(&backing_file_accessible, format!("{len}").as_bytes()).ok();

                file.set_len(mapping_size as u64)
                    .map_err(|e| e.to_string())?;
            }

            if backing_file_accessible.exists() {
                let accessible = std::fs::read_to_string(&backing_file_accessible)
                    .map_err(|e| e.to_string())?
                    .parse::<usize>()
                    .map_err(|e| e.to_string())?;
                accessible_size = accessible_size.max(accessible);
            } else {
                accessible_size = accessible_size.max(len);
            }

            accessible_size = accessible_size.min(mapping_size);
            memory_fd = file.into_raw_fd();
        }

        // Compute the flags
        let mut flags = match memory_fd {
            fd if fd < 0 => libc::MAP_ANON,
            _ => libc::MAP_FILE,
        };
        flags |= match memory_type {
            MmapType::Private => libc::MAP_PRIVATE,
            MmapType::Shared => libc::MAP_SHARED,
        };

        Ok(if accessible_size == mapping_size {
            // Allocate a single read-write region at once.
            let ptr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    mapping_size,
                    libc::PROT_READ | libc::PROT_WRITE,
                    flags,
                    memory_fd,
                    0,
                )
            };
            if ptr as isize == -1_isize {
                return Err(io::Error::last_os_error().to_string());
            }

            Self {
                ptr: ptr as usize,
                total_size: mapping_size,
                accessible_size,
                sync_on_drop: memory_fd != -1 && memory_type == MmapType::Shared,
            }
        } else {
            // Reserve the mapping size.
            let ptr = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    mapping_size,
                    libc::PROT_NONE,
                    flags,
                    memory_fd,
                    0,
                )
            };
            if ptr as isize == -1_isize {
                return Err(io::Error::last_os_error().to_string());
            }

            let mut result = Self {
                ptr: ptr as usize,
                total_size: mapping_size,
                accessible_size,
                sync_on_drop: memory_fd != -1 && memory_type == MmapType::Shared,
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
        _backing_file: Option<std::path::PathBuf>,
        _memory_type: MmapType,
    ) -> Result<Self, String> {
        use windows_sys::Win32::System::Memory::{
            VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE,
        };

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
                total_size: mapping_size,
                accessible_size,
                sync_on_drop: false,
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
                total_size: mapping_size,
                accessible_size,
                sync_on_drop: false,
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
        assert_le!(len, self.total_size);
        assert_le!(start, self.total_size - len);

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
        use std::ffi::c_void;
        use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_COMMIT, PAGE_READWRITE};
        let page_size = region::page::size();
        assert_eq!(start & (page_size - 1), 0);
        assert_eq!(len & (page_size - 1), 0);
        assert_le!(len, self.len());
        assert_le!(start, self.len() - len);

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
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.total_size) }
    }

    /// Return the allocated memory as a slice of u8.
    pub fn as_slice_accessible(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.accessible_size) }
    }

    /// Return the allocated memory as a slice of u8.
    pub fn as_slice_arbitary(&self, size: usize) -> &[u8] {
        let size = usize::min(size, self.total_size);
        unsafe { slice::from_raw_parts(self.ptr as *const u8, size) }
    }

    /// Return the allocated memory as a mutable slice of u8.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr as *mut u8, self.total_size) }
    }

    /// Return the allocated memory as a mutable slice of u8.
    pub fn as_mut_slice_accessible(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr as *mut u8, self.accessible_size) }
    }

    /// Return the allocated memory as a mutable slice of u8.
    pub fn as_mut_slice_arbitary(&mut self, size: usize) -> &mut [u8] {
        let size = usize::min(size, self.total_size);
        unsafe { slice::from_raw_parts_mut(self.ptr as *mut u8, size) }
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
        self.total_size
    }

    /// Return whether any memory has been allocated.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Duplicate in a new memory mapping.
    #[deprecated = "use `copy` instead"]
    pub fn duplicate(&mut self, size_hint: Option<usize>) -> Result<Self, String> {
        self.copy(size_hint)
    }

    /// Duplicate in a new memory mapping.
    pub fn copy(&mut self, size_hint: Option<usize>) -> Result<Self, String> {
        // NOTE: accessible_size != used size as the value is not
        //       automatically updated when the pre-provisioned space is used
        let mut copy_size = self.accessible_size;
        if let Some(size_hint) = size_hint {
            copy_size = usize::max(copy_size, size_hint);
        }

        let mut new =
            Self::accessible_reserved(copy_size, self.total_size, None, MmapType::Private)?;
        new.as_mut_slice_arbitary(copy_size)
            .copy_from_slice(self.as_slice_arbitary(copy_size));
        Ok(new)
    }
}

impl Drop for Mmap {
    #[cfg(not(target_os = "windows"))]
    fn drop(&mut self) {
        if self.total_size != 0 {
            if self.sync_on_drop {
                let r = unsafe {
                    libc::msync(
                        self.ptr as *mut libc::c_void,
                        self.total_size,
                        libc::MS_SYNC | libc::MS_INVALIDATE,
                    )
                };
                assert_eq!(r, 0, "msync failed: {}", io::Error::last_os_error());
            }
            let r = unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.total_size) };
            assert_eq!(r, 0, "munmap failed: {}", io::Error::last_os_error());
        }
    }

    #[cfg(target_os = "windows")]
    fn drop(&mut self) {
        if self.len() != 0 {
            use std::ffi::c_void;
            use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};
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
