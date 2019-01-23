use std::ops::{Deref, DerefMut};

use crate::{
    sys,
    types::{LocalMemoryIndex, Memory},
    vm,
};

/// A linear memory instance.
#[derive(Debug)]
pub struct LinearMemory {
    /// The actual memory allocation.
    memory: sys::Memory,

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
    #[doc(hidden)]
    pub const DEFAULT_HEAP_SIZE: usize = 1 << 32; // 4 GiB
    #[doc(hidden)]
    pub const DEFAULT_GUARD_SIZE: usize = 1 << 31; // 2 GiB
    pub(crate) const DEFAULT_SIZE: usize = Self::DEFAULT_HEAP_SIZE + Self::DEFAULT_GUARD_SIZE; // 6 GiB

    /// Create a new linear memory instance with specified initial and maximum number of pages.
    ///
    /// `maximum` cannot be set to more than `65536` pages.
    pub(crate) fn new(mem: &Memory) -> Self {
        assert!(mem.min <= Self::MAX_PAGES);
        assert!(mem.max.is_none() || mem.max.unwrap() <= Self::MAX_PAGES);
        debug!("Instantiate LinearMemory(mem: {:?})", mem);

        let (mmap_size, initial_pages, offset_guard_size, requires_signal_catch) = if
        /*mem.is_static_heap()*/
        true {
            (Self::DEFAULT_SIZE, mem.min, Self::DEFAULT_GUARD_SIZE, true)
        // This is a static heap
        } else {
            // this is a dynamic heap
            assert!(!mem.shared, "shared memories must have a maximum size.");

            (
                mem.min as usize * Self::PAGE_SIZE as usize,
                mem.min,
                0,
                false,
            )
        };

        let mut memory = sys::Memory::with_size(mmap_size).unwrap();

        // map initial pages as readwrite since the inital mmap is mapped as not accessible.
        if initial_pages != 0 {
            unsafe {
                memory
                    .protect(
                        0..(initial_pages as usize * Self::PAGE_SIZE as usize),
                        sys::Protect::ReadWrite,
                    )
                    .expect("unable to make memory accessible");
            }
        }

        Self {
            memory,
            current: initial_pages,
            max: mem.max,
            offset_guard_size,
            requires_signal_catch,
        }
    }

    /// Returns an base address of this linear memory.
    fn base(&mut self) -> *mut u8 {
        self.memory.as_ptr()
    }

    /// Returns the size in bytes
    pub(crate) fn size(&self) -> usize {
        self.current as usize * Self::PAGE_SIZE as usize
    }

    pub fn pages(&self) -> u32 {
        self.current
    }

    /// Returns the maximum number of wasm pages allowed.
    pub fn max(&self) -> u32 {
        self.max.unwrap_or(Self::MAX_PAGES)
    }

    pub(crate) fn into_vm_memory(&mut self, index: LocalMemoryIndex) -> vm::LocalMemory {
        vm::LocalMemory {
            base: self.base(),
            size: self.size(),
            index,
        }
    }

    /// Grow memory by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub(crate) fn grow_dynamic(&mut self, add_pages: u32) -> Option<i32> {
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

        let new_bytes = (new_pages * Self::PAGE_SIZE) as usize;

        if new_bytes > self.memory.size() - self.offset_guard_size {
            let memory_size = new_bytes.checked_add(self.offset_guard_size)?;
            let mut new_memory = sys::Memory::with_size(memory_size).ok()?;

            unsafe {
                new_memory
                    .protect(0..new_bytes, sys::Protect::ReadWrite)
                    .ok()?;
            }

            let copy_size = self.memory.size() - self.offset_guard_size;
            unsafe {
                new_memory.as_slice_mut()[..copy_size]
                    .copy_from_slice(&self.memory.as_slice()[..copy_size]);
            }

            self.memory = new_memory;
        }

        self.current = new_pages;

        Some(prev_pages as i32)
    }

    pub(crate) fn grow_static(&mut self, add_pages: u32) -> Option<i32> {
        // debug!("grow_memory_static called!");
        // assert!(self.max.is_some());
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
            self.memory
                .protect(prev_bytes..new_bytes, sys::Protect::ReadWrite)
                .ok()?;
        }

        self.current = new_pages;

        Some(prev_pages as i32)
    }
}

impl Deref for LinearMemory {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { self.memory.as_slice() }
    }
}

impl DerefMut for LinearMemory {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { self.memory.as_slice_mut() }
    }
}
