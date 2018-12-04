//! The webassembly::Memory() constructor creates a new Memory object which is
//! a structure that holds the raw bytes of memory accessed by a
//! webassembly::Instance.
//! A memory created by Rust or in WebAssembly code will be accessible and
//! mutable from both Rust and WebAssembly.
use nix::libc::{c_void, mprotect, PROT_READ, PROT_WRITE};
use nix::sys::mman::{mmap, MapFlags, ProtFlags};
use std::ops::{Deref, DerefMut};
use std::slice;

/// A linear memory instance.
//
#[derive(Debug)]
pub struct LinearMemory {
    base: *mut c_void, // The size will always be `LinearMemory::DEFAULT_SIZE`
    current: u32,      // current number of wasm pages
    // The maximum size the WebAssembly Memory is allowed to grow
    // to, in units of WebAssembly pages.  When present, the maximum
    // parameter acts as a hint to the engine to reserve memory up
    // front.  However, the engine may ignore or clamp this reservation
    // request.  In general, most WebAssembly modules shouldn't need
    // to set a maximum.
    maximum: Option<u32>,
}

/// It holds the raw bytes of memory accessed by a WebAssembly Instance
impl LinearMemory {
    pub const PAGE_SIZE: u32 = 65536;
    pub const MAX_PAGES: u32 = 65536;
    pub const WASM_PAGE_SIZE: usize = 1 << 16; // 64 KiB
    pub const DEFAULT_HEAP_SIZE: usize = 1 << 32; // 4 GiB
    pub const DEFAULT_GUARD_SIZE: usize = 1 << 31; // 2 GiB
    pub const DEFAULT_SIZE: usize = Self::DEFAULT_HEAP_SIZE + Self::DEFAULT_GUARD_SIZE; // 6 GiB

    /// Create a new linear memory instance with specified initial and maximum number of pages.
    ///
    /// `maximum` cannot be set to more than `65536` pages.
    pub fn new(initial: u32, maximum: Option<u32>) -> Self {
        assert!(initial <= Self::MAX_PAGES);
        assert!(maximum.is_none() || maximum.unwrap() <= Self::MAX_PAGES);
        debug!(
            "Instantiate LinearMemory(initial={:?}, maximum={:?})",
            initial, maximum
        );

        // TODO: Investigate if memory is zeroed out
        let base = unsafe {
            mmap(
                0 as _,
                LinearMemory::DEFAULT_SIZE,
                ProtFlags::PROT_NONE,
                MapFlags::MAP_ANON | MapFlags::MAP_PRIVATE,
                -1,
                0,
            ).unwrap()
        };

        if initial > 0 {
            assert_eq!(
                unsafe {
                    mprotect(
                        base,
                        initial as usize * Self::PAGE_SIZE as usize,
                        // Self::DEFAULT_HEAP_SIZE,
                        PROT_READ | PROT_WRITE,
                    )
                },
                0
            );
        }
    
        debug!("LinearMemory instantiated");
        debug!("  - usable: {:#x}..{:#x}", base as usize, (base as usize) + LinearMemory::DEFAULT_HEAP_SIZE);
        debug!("  - guard: {:#x}..{:#x}", (base as usize) + LinearMemory::DEFAULT_HEAP_SIZE, (base as usize) + LinearMemory::DEFAULT_SIZE);
        Self {
            base,
            current: initial,
            maximum,
        }
    }

    /// Returns an base address of this linear memory.
    pub fn base_addr(&mut self) -> *mut u8 {
        self.base as _
    }

    /// Returns a number of allocated wasm pages.
    pub fn current_size(&self) -> usize {
        self.current as usize * Self::PAGE_SIZE as usize
    }

    pub fn current_pages(&self) -> u32 {
        self.current
    }

    /// Returns the maximum number of wasm pages allowed.
    pub fn maximum_size(&self) -> u32 {
        self.maximum.unwrap_or(65536)
    }

    /// Grow memory by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub fn grow(&mut self, add_pages: u32) -> Option<i32> {
        debug!("grow_memory called!");
        if add_pages == 0 {
            return Some(self.current as _);
        }

        let prev_pages = self.current;

        let new_pages = match self.current.checked_add(add_pages) {
            Some(new_pages) => new_pages,
            None => return None,
        };

        if let Some(val) = self.maximum {
            if new_pages > val {
                return None;
            }
        // Wasm linear memories are never allowed to grow beyond what is
        // indexable. If the memory has no maximum, enforce the greatest
        // limit here.
        } else if new_pages >= Self::MAX_PAGES {
            return None;
        }

        let prev_bytes = (prev_pages * Self::PAGE_SIZE) as usize;
        let new_bytes = (new_pages * Self::PAGE_SIZE) as usize;

        unsafe {
            assert_eq!(
                mprotect(
                    self.base.add(prev_bytes),
                    new_bytes - prev_bytes,
                    PROT_READ | PROT_WRITE,
                ),
                0
            );
        }

        self.current = new_pages;

        Some(prev_pages as i32)
    }

    pub fn carve_slice(&self, offset: u32, size: u32) -> Option<&[u8]> {
        let start = offset as usize;
        let end = start + size as usize;
        let slice: &[u8] = &*self;

        if end <= self.current_size() as usize {
            Some(&slice[start..end])
        } else {
            None
        }
    }
}

// Not comparing based on memory content. That would be inefficient.
impl PartialEq for LinearMemory {
    fn eq(&self, other: &LinearMemory) -> bool {
        self.current == other.current && self.maximum == other.maximum
    }
}

impl Deref for LinearMemory {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.base as _, self.current as usize * Self::PAGE_SIZE as usize) }
    }
}

impl DerefMut for LinearMemory {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.base as _, self.current as usize * Self::PAGE_SIZE as usize) }
    }
}
