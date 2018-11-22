//! The webassembly::Memory() constructor creates a new Memory object which is
//! a structure that holds the raw bytes of memory accessed by a
//! webassembly::Instance.
//! A memory created by Rust or in WebAssembly code will be accessible and
//! mutable from both Rust and WebAssembly.
use memmap::MmapMut;
use std::fmt;
use std::ops::{Deref, DerefMut};

const PAGE_SIZE: u32 = 65536;
const MAX_PAGES: u32 = 65536;

/// A linear memory instance.
///
pub struct LinearMemory {
    pub mmap: MmapMut,
    // The initial size of the WebAssembly Memory, in units of
    // WebAssembly pages.
    pub current: u32,
    // The maximum size the WebAssembly Memory is allowed to grow
    // to, in units of WebAssembly pages.  When present, the maximum
    // parameter acts as a hint to the engine to reserve memory up
    // front.  However, the engine may ignore or clamp this reservation
    // request.  In general, most WebAssembly modules shouldn't need
    // to set a maximum.
    pub maximum: Option<u32>,
}

/// It holds the raw bytes of memory accessed by a WebAssembly Instance
impl LinearMemory {
    pub const WASM_PAGE_SIZE: usize = 1 << 16; // 64 KiB
    pub const DEFAULT_HEAP_SIZE: usize = 1 << 32; // 4 GiB
    pub const DEFAULT_GUARD_SIZE: usize = 1 << 31; // 2 GiB
    pub const DEFAULT_SIZE: usize = Self::DEFAULT_HEAP_SIZE + Self::DEFAULT_GUARD_SIZE; // 6 GiB

    /// Create a new linear memory instance with specified initial and maximum number of pages.
    ///
    /// `maximum` cannot be set to more than `65536` pages.
    pub fn new(initial: u32, maximum: Option<u32>) -> Self {
        assert!(initial <= MAX_PAGES);
        assert!(maximum.is_none() || maximum.unwrap() <= MAX_PAGES);
        debug!(
            "Instantiate LinearMemory(initial={:?}, maximum={:?})",
            initial, maximum
        );

        // TODO: Investigate if memory is zeroed out
        let mmap = MmapMut::map_anon(LinearMemory::DEFAULT_HEAP_SIZE).unwrap();

        debug!("LinearMemory instantiated");
        Self {
            mmap,
            current: initial,
            maximum,
        }
    }

    /// Returns an base address of this linear memory.
    pub fn base_addr(&mut self) -> *mut u8 {
        self.mmap.as_mut_ptr()
    }

    /// Returns a number of allocated wasm pages.
    pub fn current_size(&self) -> u32 {
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
        debug!("old memory = {:?}", self.mmap);
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
        } else if new_pages >= 65536 {
            return None;
        }

        let prev_bytes = (prev_pages * PAGE_SIZE) as usize;
        let new_bytes = (new_pages * PAGE_SIZE) as usize;

        // Updating self.current if new_bytes > prev_bytes
        if new_bytes > prev_bytes {
            self.current = new_pages;
        }

        Some(prev_pages as i32)
    }

    pub fn carve_slice(&self, offset: u32, size: u32) -> Option<&[u8]> {
        let start = offset as usize;
        let end = start + size as usize;
        let slice: &[u8] = &*self;

        // if end <= self.mapped_size() {
        Some(&slice[start..end])
        // } else {
        //     None
        // }
    }
}

impl fmt::Debug for LinearMemory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LinearMemory")
            .field("mmap", &self.mmap)
            .field("current", &self.current)
            .field("maximum", &self.maximum)
            .finish()
    }
}

// Not comparing based on memory content. That would be inefficient.
impl PartialEq for LinearMemory {
    fn eq(&self, other: &LinearMemory) -> bool {
        self.current == other.current && self.maximum == other.maximum
    }
}

impl AsRef<[u8]> for LinearMemory {
    fn as_ref(&self) -> &[u8] {
        &self.mmap
    }
}

impl AsMut<[u8]> for LinearMemory {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.mmap
    }
}

impl Deref for LinearMemory {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &*self.mmap
    }
}

impl DerefMut for LinearMemory {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut *self.mmap
    }
}

// impl Clone for LinearMemory {
//     fn clone(&self) -> LinearMemory {
//         let mut mmap = MmapMut::map_anon(self.maximum.unwrap_or(self.current) as usize).unwrap();
//         let mut base_mmap = &self.mmap;
//         let to_init = &mut mmap[0..self.current as usize];
//         to_init.copy_from_slice(&self.mmap);

//         return LinearMemory {
//             mmap: mmap,
//             current: self.current,
//             maximum: self.maximum,
//         };
//     }
// }
