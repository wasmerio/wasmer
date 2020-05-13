//! Memory management for linear memories.
//!
//! `LinearMemory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use crate::mmap::Mmap;
use crate::module::{MemoryPlan, MemoryStyle};
use crate::vmcontext::VMMemoryDefinition;
use more_asserts::{assert_ge, assert_le};
use std::cell::RefCell;
use thiserror::Error;
use wasm_common::{Bytes, Pages};

/// Error type describing things that can go wrong when operating on Wasm Memories.
#[derive(Error, Debug, Clone, PartialEq, Hash)]
pub enum MemoryError {
    /// Low level error with mmap.
    #[error("There was an error when `mmap`ing memory: {0}")]
    MmapError(String),
    /// The operation would cause the size of the memory size to overflow or otherwise
    /// create memory that is not indexable.

    #[error("Memory address space overflow: current size: {}, requested increase: {}", current.0, attempted_delta.0)]
    SizeOverflow {
        /// The current size in pages.
        current: Pages,
        /// The attempted amount to grow by in pages.
        attempted_delta: Pages,
    },
    /// The operation would cause the size of the memory size exceed the maximum.
    #[error("Maximum memory size exceeded: current size: {}, requested increase: {}, maximum: {}", current.0, attempted_delta.0, maximum.0)]
    SizeExceeded {
        /// The current size in pages.
        current: Pages,
        /// The attempted amount to grow by in pages.
        attempted_delta: Pages,
        /// The maximum number of pages this memory may have.
        maximum: Pages,
    },
    /// The operation would cause the size of the memory size exceed the maximum.
    #[error("The given memory plan was invalid because {}", reason)]
    InvalidMemoryPlan {
        /// The reason why the memory plan is invalid.
        reason: String,
    },
    // TODO: review this; consider alternatives
    /// A user defined error value, used for error cases not listed above.
    #[error("A user-defined error occurred: {0}")]
    Other(String),
}

/// A linear memory instance.
#[derive(Debug)]
pub struct LinearMemory {
    // The underlying allocation.
    mmap: RefCell<WasmMmap>,

    // The optional maximum size in wasm pages of this linear memory.
    maximum: Option<Pages>,

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
    size: Pages,
}

impl LinearMemory {
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    pub fn new(plan: &MemoryPlan) -> Result<Self, MemoryError> {
        // `maximum` cannot be set to more than `65536` pages.
        assert_le!(plan.memory.minimum, Pages::max_value());
        assert!(
            plan.memory.maximum.is_none() || plan.memory.maximum.unwrap() <= Pages::max_value()
        );

        if plan.memory.maximum.is_some() && plan.memory.maximum.unwrap() < plan.memory.minimum {
            return Err(MemoryError::InvalidMemoryPlan {
                reason: format!("the maximum allowed memory ({} pages) is less than the minimum required memory ({} pages)", plan.memory.maximum.unwrap().0, plan.memory.minimum.0),
            });
        }

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
        };
        let minimum_bytes = minimum_pages.bytes().0;
        let request_bytes = minimum_bytes.checked_add(offset_guard_bytes).unwrap();
        let mapped_pages = plan.memory.minimum;
        let mapped_bytes = mapped_pages.bytes();

        let mmap = WasmMmap {
            alloc: Mmap::accessible_reserved(mapped_bytes.0, request_bytes)
                .map_err(MemoryError::MmapError)?,
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
    pub fn size(&self) -> Pages {
        self.mmap.borrow().size
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    pub fn grow<IntoPages>(&self, delta: IntoPages) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        // Optimization of memory.grow 0 calls.
        let delta: Pages = delta.into();
        let mut mmap = self.mmap.borrow_mut();
        if delta.0 == 0 {
            return Ok(mmap.size);
        }

        let new_pages = mmap
            .size
            .checked_add(delta)
            .ok_or_else(|| MemoryError::SizeOverflow {
                current: mmap.size,
                attempted_delta: delta,
            })?;
        let prev_pages = mmap.size;

        if let Some(maximum) = self.maximum {
            if new_pages > maximum {
                return Err(MemoryError::SizeExceeded {
                    current: mmap.size,
                    attempted_delta: delta,
                    maximum,
                });
            }
        }

        // Wasm linear memories are never allowed to grow beyond what is
        // indexable. If the memory has no maximum, enforce the greatest
        // limit here.
        if new_pages >= Pages::max_value() {
            // Linear memory size would exceed the index range.
            return Err(MemoryError::SizeOverflow {
                current: mmap.size,
                attempted_delta: delta,
            });
        }

        let delta_bytes = delta.bytes().0;
        let prev_bytes = prev_pages.bytes().0;
        let new_bytes = new_pages.bytes().0;

        if new_bytes > mmap.alloc.len() - self.offset_guard_size {
            // If the new size is within the declared maximum, but needs more memory than we
            // have on hand, it's a dynamic heap and it can move.
            let guard_bytes = self.offset_guard_size;
            let request_bytes =
                new_bytes
                    .checked_add(guard_bytes)
                    .ok_or_else(|| MemoryError::SizeOverflow {
                        current: new_pages,
                        attempted_delta: Bytes(guard_bytes).into(),
                    })?;

            let mut new_mmap = Mmap::accessible_reserved(new_bytes, request_bytes)
                .map_err(MemoryError::MmapError)?;

            let copy_len = mmap.alloc.len() - self.offset_guard_size;
            new_mmap.as_mut_slice()[..copy_len].copy_from_slice(&mmap.alloc.as_slice()[..copy_len]);

            mmap.alloc = new_mmap;
        } else if delta_bytes > 0 {
            // Make the newly allocated pages accessible.
            mmap.alloc
                .make_accessible(prev_bytes, delta_bytes)
                .map_err(MemoryError::MmapError)?;
        }

        mmap.size = new_pages;

        Ok(prev_pages)
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    pub fn vmmemory(&self) -> VMMemoryDefinition {
        let mut mmap = self.mmap.borrow_mut();
        VMMemoryDefinition {
            base: mmap.alloc.as_mut_ptr(),
            current_length: mmap.size.bytes().0,
        }
    }

    /// Get the host memory as mutable pointer
    ///
    /// This function is used in the `wasmer_runtime::Instance` to retrieve
    /// the host memory pointer and interact with the host memory directly.
    pub fn as_mut_ptr(&self) -> *mut LinearMemory {
        self as *const LinearMemory as *mut LinearMemory
    }
}
