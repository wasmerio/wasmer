//! The webassembly::Memory() constructor creates a new Memory object which is
//! a structure that holds the raw bytes of memory accessed by a
//! webassembly::Instance.
//! A memory created by Rust or in WebAssembly code will be accessible and
//! mutable from both Rust and WebAssembly.
use region;
use std::ops::{Deref, DerefMut};
use std::slice;

use crate::common::mmap::Mmap;

/// A linear memory instance.
#[derive(Debug)]
pub struct LinearMemory {
    // The mmap allocation
    mmap: Mmap,

    // current number of wasm pages
    current: u32,

    // The maximum size the WebAssembly Memory is allowed to grow
    // to, in units of WebAssembly pages.  When present, the maximum
    // parameter acts as a hint to the engine to reserve memory up
    // front.  However, the engine may ignore or clamp this reservation
    // request.  In general, most WebAssembly modules shouldn't need
    // to set a maximum.
    maximum: Option<u32>,

    // The size of the extra guard pages after the end.
    // Is used to optimize loads and stores with constant offsets.
    offset_guard_size: usize,
}

/// It holds the raw bytes of memory accessed by a WebAssembly Instance
impl LinearMemory {
    pub const PAGE_SIZE: u32 = 65536;
    pub const MAX_PAGES: u32 = 65536;
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

        let mut mmap = Mmap::with_size(Self::DEFAULT_SIZE).expect("Can't create mmap");

        let base = mmap.as_mut_ptr();

        // map initial pages as readwrite since the inital mmap is mapped as not accessible.
        if initial != 0 {
            unsafe {
                region::protect(
                    base,
                    initial as usize * Self::PAGE_SIZE as usize,
                    region::Protection::ReadWrite,
                )
            }
            .expect("unable to make memory inaccessible");
        }

        debug!("LinearMemory instantiated");
        debug!(
            "  - usable: {:#x}..{:#x}",
            base as usize,
            (base as usize) + LinearMemory::DEFAULT_HEAP_SIZE
        );
        debug!(
            "  - guard: {:#x}..{:#x}",
            (base as usize) + LinearMemory::DEFAULT_HEAP_SIZE,
            (base as usize) + LinearMemory::DEFAULT_SIZE
        );
        Self {
            mmap,
            current: initial,
            offset_guard_size: LinearMemory::DEFAULT_GUARD_SIZE,
            maximum,
        }
    }

    /// Returns an base address of this linear memory.
    pub fn base(&mut self) -> *mut u8 {
        self.mmap.as_mut_ptr() as _
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
        self.maximum.unwrap_or(Self::MAX_PAGES)
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

        // if new_bytes > self.mmap.len() - self.offset_guard_size {
        unsafe {
            region::protect(
                self.mmap.as_ptr().add(prev_bytes) as _,
                new_bytes - prev_bytes,
                region::Protection::ReadWrite,
            )
        }
        .expect("unable to make memory inaccessible");
        // };
        // if new_bytes > self.mmap.len() - self.offset_guard_size {
        //     // If we have no maximum, this is a "dynamic" heap, and it's allowed to move.
        //     assert!(self.maximum.is_none());
        //     let guard_bytes = self.offset_guard_size;
        //     let request_bytes = new_bytes.checked_add(guard_bytes)?;

        //     let mut new_mmap = Mmap::with_size(request_bytes).ok()?;

        //     // Make the offset-guard pages inaccessible.
        //     unsafe {
        //         region::protect(
        //             new_mmap.as_ptr().add(new_bytes),
        //             guard_bytes,
        //             region::Protection::Read | region::Protection::Write,
        //             // region::Protection::None,
        //         )
        //     }
        //     .expect("unable to make memory inaccessible");

        //     let copy_len = self.mmap.len() - self.offset_guard_size;
        //     new_mmap.as_mut_slice()[..copy_len].copy_from_slice(&self.mmap.as_slice()[..copy_len]);

        //     self.mmap = new_mmap;
        // }

        self.current = new_pages;

        Some(prev_pages as i32)
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
        unsafe {
            slice::from_raw_parts(
                self.mmap.as_ptr() as _,
                self.current as usize * Self::PAGE_SIZE as usize,
            )
        }
    }
}

impl DerefMut for LinearMemory {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self.mmap.as_mut_ptr() as _,
                self.current as usize * Self::PAGE_SIZE as usize,
            )
        }
    }
}
