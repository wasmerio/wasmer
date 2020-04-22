//! Memory management for linear memories.
//!
//! `LinearMemory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use crate::mmap::Mmap;
use crate::module::{MemoryPlan, MemoryStyle};
use crate::vmcontext::VMMemoryDefinition;
use more_asserts::{assert_ge, assert_le};
use std::cell::RefCell;
use std::convert::TryFrom;
use wasm_common::{WASM_MAX_PAGES, WASM_PAGE_SIZE};

/// A linear memory instance.
#[derive(Debug)]
pub struct LinearMemory {
    // The underlying allocation.
    mmap: RefCell<WasmMmap>,

    // The optional maximum size in wasm pages of this linear memory.
    maximum: Option<u32>,

    // Size in bytes of extra guard pages after the end to optimize loads and stores with
    // constant offsets.
    offset_guard_size: usize,

    // The memory plan for this memory
    plan: MemoryPlan,

    // Records whether we're using a bounds-checking strategy which requires
    // handlers to catch trapping accesses.
    pub(crate) needs_signal_handlers: bool,
}

#[derive(Debug)]
struct WasmMmap {
    // Our OS allocation of mmap'd memory.
    alloc: Mmap,
    // The current logical size in wasm pages of this linear memory.
    size: u32,
}

impl LinearMemory {
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    pub fn new(plan: &MemoryPlan) -> Result<Self, String> {
        // `maximum` cannot be set to more than `65536` pages.
        assert_le!(plan.memory.minimum, WASM_MAX_PAGES);
        assert!(plan.memory.maximum.is_none() || plan.memory.maximum.unwrap() <= WASM_MAX_PAGES);

        let offset_guard_bytes = plan.offset_guard_size as usize;

        // If we have an offset guard, or if we're doing the static memory
        // allocation strategy, we need signal handlers to catch out of bounds
        // acceses.
        let needs_signal_handlers = offset_guard_bytes > 0
            || match plan.style {
                MemoryStyle::Dynamic => false,
                MemoryStyle::Static { .. } => true,
            };

        let minimum_pages = match plan.style {
            MemoryStyle::Dynamic => plan.memory.minimum,
            MemoryStyle::Static { bound } => {
                assert_ge!(bound, plan.memory.minimum);
                bound
            }
        } as usize;
        let minimum_bytes = minimum_pages.checked_mul(WASM_PAGE_SIZE as usize).unwrap();
        let request_bytes = minimum_bytes.checked_add(offset_guard_bytes).unwrap();
        let mapped_pages = plan.memory.minimum as usize;
        let mapped_bytes = mapped_pages * WASM_PAGE_SIZE as usize;

        let mmap = WasmMmap {
            alloc: Mmap::accessible_reserved(mapped_bytes, request_bytes)?,
            size: plan.memory.minimum,
        };

        Ok(Self {
            mmap: mmap.into(),
            maximum: plan.memory.maximum,
            offset_guard_size: offset_guard_bytes,
            needs_signal_handlers,
            plan: plan.clone(),
        })
    }

    /// Returns the memory plan for this memory.
    pub fn plan(&self) -> &MemoryPlan {
        &self.plan
    }

    /// Returns the number of allocated wasm pages.
    pub fn size(&self) -> u32 {
        self.mmap.borrow().size
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    pub fn grow(&self, delta: u32) -> Option<u32> {
        // Optimization of memory.grow 0 calls.
        let mut mmap = self.mmap.borrow_mut();
        if delta == 0 {
            return Some(mmap.size);
        }

        let new_pages = match mmap.size.checked_add(delta) {
            Some(new_pages) => new_pages,
            // Linear memory size overflow.
            None => return None,
        };
        let prev_pages = mmap.size;

        if let Some(maximum) = self.maximum {
            if new_pages > maximum {
                // Linear memory size would exceed the declared maximum.
                return None;
            }
        }

        // Wasm linear memories are never allowed to grow beyond what is
        // indexable. If the memory has no maximum, enforce the greatest
        // limit here.
        if new_pages >= WASM_MAX_PAGES {
            // Linear memory size would exceed the index range.
            return None;
        }

        let delta_bytes = usize::try_from(delta).unwrap() * WASM_PAGE_SIZE as usize;
        let prev_bytes = usize::try_from(prev_pages).unwrap() * WASM_PAGE_SIZE as usize;
        let new_bytes = usize::try_from(new_pages).unwrap() * WASM_PAGE_SIZE as usize;

        if new_bytes > mmap.alloc.len() - self.offset_guard_size {
            // If the new size is within the declared maximum, but needs more memory than we
            // have on hand, it's a dynamic heap and it can move.
            let guard_bytes = self.offset_guard_size;
            let request_bytes = new_bytes.checked_add(guard_bytes)?;

            let mut new_mmap = Mmap::accessible_reserved(new_bytes, request_bytes).ok()?;

            let copy_len = mmap.alloc.len() - self.offset_guard_size;
            new_mmap.as_mut_slice()[..copy_len].copy_from_slice(&mmap.alloc.as_slice()[..copy_len]);

            mmap.alloc = new_mmap;
        } else if delta_bytes > 0 {
            // Make the newly allocated pages accessible.
            mmap.alloc.make_accessible(prev_bytes, delta_bytes).ok()?;
        }

        mmap.size = new_pages;

        Some(prev_pages)
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    pub fn vmmemory(&self) -> VMMemoryDefinition {
        let mut mmap = self.mmap.borrow_mut();
        VMMemoryDefinition {
            base: mmap.alloc.as_mut_ptr(),
            current_length: mmap.size as usize * WASM_PAGE_SIZE as usize,
        }
    }

    /// Get the memory host as mutable pointer
    ///
    /// This function is used in the `wasmer_runtime::Instance` to retrieve
    /// the host memory pointer and interact with the host memory directly.
    pub fn as_mut_ptr(&self) -> *mut LinearMemory {
        self as *const LinearMemory as *mut LinearMemory
    }
}
