use memmap::MmapMut;
use std::fmt;

const PAGE_SIZE: u32 = 65536;
const MAX_PAGES: u32 = 65536;

/// A linear memory instance.
///
/// This linear memory has a stable base address and at the same time allows
/// for dynamical growing.
pub struct LinearMemory {
    mmap: MmapMut,
    // The initial size of the WebAssembly Memory, in units of
    // WebAssembly pages.
    current: u32,
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
        let initial = if initial > 0 { initial } else { 1 };

        let len: u64 = PAGE_SIZE as u64 * match maximum {
            Some(val) => {
                if val > initial {
                    val as u64
                } else {
                    initial as u64
                }
            }
            None => initial as u64,
        };
        let len = if len == 0 { 1 } else { len };

        let mmap = MmapMut::map_anon(len as usize).unwrap();
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

    /// Grow memory by the specified amount of pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of pages.
    pub fn grow(&mut self, add_pages: u32) -> Option<u32> {
        let new_pages = match self.current.checked_add(add_pages) {
            Some(new_pages) => new_pages,
            None => return None,
        };
        if let Some(val) = self.maximum {
            if new_pages > val {
                return None;
            }
        } else {
            // Wasm linear memories are never allowed to grow beyond what is
            // indexable. If the memory has no maximum, enforce the greatest
            // limit here.
            if new_pages >= 65536 {
                return None;
            }
        }

        let prev_pages = self.current;
        let new_bytes = (new_pages * PAGE_SIZE) as usize;

        if self.mmap.len() < new_bytes {
            // If we have no maximum, this is a "dynamic" heap, and it's allowed
            // to move.
            assert!(self.maximum.is_none());
            let mut new_mmap = MmapMut::map_anon(new_bytes).unwrap();
            new_mmap.copy_from_slice(&self.mmap);
            self.mmap = new_mmap;
        }

        self.current = new_pages;

        // Ensure that newly allocated area is zeroed.
        let new_start_offset = (prev_pages * PAGE_SIZE) as usize;
        let new_end_offset = (new_pages * PAGE_SIZE) as usize;
        for i in new_start_offset..new_end_offset {
            assert!(self.mmap[i] == 0);
        }

        Some(prev_pages)
    }
}

impl fmt::Debug for LinearMemory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("LinearMemory")
            .field("current", &self.current)
            .field("maximum", &self.maximum)
            .finish()
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
