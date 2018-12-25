//! The webassembly::Memory() constructor creates a new Memory object which is
//! a structure that holds the raw bytes of memory accessed by a
//! webassembly::Instance.
//! A memory created by Rust or in WebAssembly code will be accessible and
//! mutable from both Rust and WebAssembly.
use region;
use std::ops::{Deref, DerefMut};
use std::slice;

use crate::common::mmap::Mmap;
use crate::runtime::{
    vm::LocalMemory,
    types::Memory,
};

/// A linear memory instance.
#[derive(Debug)]
pub struct LinearMemory {
    /// The actual memory allocation.
    mmap: Mmap,

    /// The current number of wasm pages.
    current: u32,

    // The maximum size the WebAssembly Memory is allowed to grow
    // to, in units of WebAssembly pages.  When present, the maximum
    // parameter acts as a hint to the engine to reserve memory up
    // front.  However, the engine may ignore or clamp this reservation
    // request.  In general, most WebAssembly modules shouldn't need
    // to set a maximum.
    max: Option<u32>,

    // The size of the extra guard pages after the end.
    // Is used to optimize loads and stores with constant offsets.
    offset_guard_size: usize,

    /// Requires exception catching to handle out-of-bounds accesses.
    requires_signal_catch: bool,
}

/// It holds the raw bytes of memory accessed by a WebAssembly Instance
impl LinearMemory {
    pub const PAGE_SIZE: u32 = 65_536;
    pub const MAX_PAGES: u32 = 65_536;
    pub const DEFAULT_HEAP_SIZE: usize = 1 << 32; // 4 GiB
    pub const DEFAULT_GUARD_SIZE: usize = 1 << 31; // 2 GiB
    pub const DEFAULT_SIZE: usize = Self::DEFAULT_HEAP_SIZE + Self::DEFAULT_GUARD_SIZE; // 6 GiB

    /// Create a new linear memory instance with specified initial and maximum number of pages.
    ///
    /// `maximum` cannot be set to more than `65536` pages.
    pub fn new(mem: &Memory) -> Self {
        assert!(mem.min <= Self::MAX_PAGES);
        assert!(mem.max.is_none() || mem.max.unwrap() <= Self::MAX_PAGES);
        debug!("Instantiate LinearMemory(mem: {:?})", mem);

        let (mmap_size, initial_pages, offset_guard_size, requires_signal_catch) = if mem.is_static_heap() {
            (Self::DEFAULT_SIZE, mem.min, Self::DEFAULT_GUARD_SIZE, true)
            // This is a static heap
        } else {
            // this is a dynamic heap
            assert!(!mem.shared, "shared memories must have a maximum size.");

            (mem.min as usize * Self::PAGE_SIZE as usize, mem.min, 0, false)
        };

        let mut mmap = Mmap::with_size(mmap_size).unwrap();

        // map initial pages as readwrite since the inital mmap is mapped as not accessible.
        if initial_pages != 0 {
            unsafe {
                region::protect(
                    mmap.as_mut_ptr(),
                    initial_pages as usize * Self::PAGE_SIZE as usize,
                    region::Protection::ReadWrite,
                )
            }
            .expect("unable to make memory accessible");
        }

        Self {
            mmap,
            current: initial_pages,
            max: mem.max,
            offset_guard_size,
            requires_signal_catch,
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
        self.max.unwrap_or(Self::MAX_PAGES)
    }

    pub fn into_vm_memory(&mut self) -> LocalMemory {
        LocalMemory {
            base: self.base(),
            size: self.current_size(),
        }
    }

    /// Grow memory by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub fn grow_dynamic(&mut self, add_pages: u32) -> Option<i32> {
        debug!("grow_memory_dynamic called!");
        assert!(self.max.is_none());
        if add_pages == 0 {
            return Some(self.current as _);
        }

        let prev_pages = self.current;

        let new_pages = match self.current.checked_add(add_pages) {
            Some(new_pages) => new_pages,
            None => return None,
        };

        if let Some(val) = self.max {
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

        if new_bytes > self.mmap.len() - self.offset_guard_size {
            let mmap_size = new_bytes.checked_add(self.offset_guard_size)?;
            let mut new_mmap = Mmap::with_size(mmap_size).ok()?;

            unsafe {
                region::protect(
                    new_mmap.as_mut_ptr(),
                    new_bytes,
                    region::Protection::ReadWrite,
                ).ok()?;
            }

            let copy_size = self.mmap.len() - self.offset_guard_size;
            new_mmap.as_mut_slice()[..copy_size].copy_from_slice(&self.mmap.as_slice()[..copy_size]);

            self.mmap = new_mmap;
        }

        self.current = new_pages;

        Some(prev_pages as i32)
    }

    pub fn grow_static(&mut self, add_pages: u32) -> Option<i32> {
        debug!("grow_memory_static called!");
        assert!(self.max.is_some());
        if add_pages == 0 {
            return Some(self.current as _);
        }

        let prev_pages = self.current;

        let new_pages = match self.current.checked_add(add_pages) {
            Some(new_pages) => new_pages,
            None => return None,
        };

        if let Some(val) = self.max {
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
            region::protect(
                self.mmap.as_ptr().add(prev_bytes) as _,
                new_bytes - prev_bytes,
                region::Protection::ReadWrite,
            ).ok()?;
        }

        self.current = new_pages;

        Some(prev_pages as i32)
    }
}

// Not comparing based on memory content. That would be inefficient.
impl PartialEq for LinearMemory {
    fn eq(&self, other: &LinearMemory) -> bool {
        self.current == other.current && self.max == other.max
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