use crate::{Bytes, Pages};
use more_asserts::{assert_ge, assert_le};
use std::cell::RefCell;
use wasmer_runtime::{Memory, MemoryError, MemoryPlan, MemoryStyle, Mmap, VMMemoryDefinition};

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
                reason: format!(
                    "the maximum ({} pages) is less than the minimum ({} pages)",
                    plan.memory.maximum.unwrap().0,
                    plan.memory.minimum.0
                ),
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
                .map_err(MemoryError::Region)?,
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
}

impl Memory for LinearMemory {
    /// Returns the memory plan for this memory.
    fn plan(&self) -> &MemoryPlan {
        &self.plan
    }

    /// Returns the number of allocated wasm pages.
    fn size(&self) -> Pages {
        self.mmap.borrow().size
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&self, delta: Pages) -> Result<Pages, MemoryError> {
        // Optimization of memory.grow 0 calls.
        let delta: Pages = delta.into();
        let mut mmap = self.mmap.borrow_mut();
        if delta.0 == 0 {
            return Ok(mmap.size);
        }

        let new_pages = mmap
            .size
            .checked_add(delta)
            .ok_or_else(|| MemoryError::CouldNotGrow {
                current: mmap.size,
                attempted_delta: delta,
            })?;
        let prev_pages = mmap.size;

        if let Some(maximum) = self.maximum {
            if new_pages > maximum {
                return Err(MemoryError::CouldNotGrow {
                    current: mmap.size,
                    attempted_delta: delta,
                });
            }
        }

        // Wasm linear memories are never allowed to grow beyond what is
        // indexable. If the memory has no maximum, enforce the greatest
        // limit here.
        if new_pages >= Pages::max_value() {
            // Linear memory size would exceed the index range.
            return Err(MemoryError::CouldNotGrow {
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
                    .ok_or_else(|| MemoryError::CouldNotGrow {
                        current: new_pages,
                        attempted_delta: Bytes(guard_bytes).into(),
                    })?;

            let mut new_mmap =
                Mmap::accessible_reserved(new_bytes, request_bytes).map_err(MemoryError::Region)?;

            let copy_len = mmap.alloc.len() - self.offset_guard_size;
            new_mmap.as_mut_slice()[..copy_len].copy_from_slice(&mmap.alloc.as_slice()[..copy_len]);

            mmap.alloc = new_mmap;
        } else if delta_bytes > 0 {
            // Make the newly allocated pages accessible.
            mmap.alloc
                .make_accessible(prev_bytes, delta_bytes)
                .map_err(MemoryError::Region)?;
        }

        mmap.size = new_pages;

        Ok(prev_pages)
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> VMMemoryDefinition {
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
    fn as_mut_ptr(&self) -> *mut u8 {
        self as *const Self as *mut u8
    }
}
