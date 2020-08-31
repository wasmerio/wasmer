// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! Memory management for linear memories.
//!
//! `LinearMemory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use crate::mmap::Mmap;
use crate::vmcontext::VMMemoryDefinition;
use more_asserts::{assert_ge, assert_le};
use serde::{Deserialize, Serialize};
use std::borrow::BorrowMut;
use std::cell::UnsafeCell;
use std::convert::TryInto;
use std::fmt;
use std::ptr::NonNull;
use std::sync::Mutex;
use thiserror::Error;
use wasmer_types::{Bytes, MemoryType, Pages};

/// Error type describing things that can go wrong when operating on Wasm Memories.
#[derive(Error, Debug, Clone, PartialEq, Hash)]
pub enum MemoryError {
    /// Low level error with mmap.
    #[error("Error when allocating memory: {0}")]
    Region(String),
    /// The operation would cause the size of the memory to exceed the maximum or would cause
    /// an overflow leading to unindexable memory.
    #[error("The memory could not grow: current size {} pages, requested increase: {} pages", current.0, attempted_delta.0)]
    CouldNotGrow {
        /// The current size in pages.
        current: Pages,
        /// The attempted amount to grow by in pages.
        attempted_delta: Pages,
    },
    /// The operation would cause the size of the memory size exceed the maximum.
    #[error("The memory is invalid because {}", reason)]
    InvalidMemory {
        /// The reason why the provided memory is invalid.
        reason: String,
    },
    /// A user defined error value, used for error cases not listed above.
    #[error("A user-defined error occurred: {0}")]
    Generic(String),
}

/// Implementation styles for WebAssembly linear memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryStyle {
    /// The actual memory can be resized and moved.
    Dynamic {
        /// Our chosen offset-guard size.
        ///
        /// It represents the size in bytes of extra guard pages after the end
        /// to optimize loads and stores with constant offsets.
        offset_guard_size: u64,
    },
    /// Address space is allocated up front.
    Static {
        /// The number of mapped and unmapped pages.
        bound: Pages,
        /// Our chosen offset-guard size.
        ///
        /// It represents the size in bytes of extra guard pages after the end
        /// to optimize loads and stores with constant offsets.
        offset_guard_size: u64,
    },
}

impl MemoryStyle {
    /// Returns the offset-guard size
    pub fn offset_guard_size(&self) -> u64 {
        match self {
            Self::Dynamic { offset_guard_size } => *offset_guard_size,
            Self::Static {
                offset_guard_size, ..
            } => *offset_guard_size,
        }
    }
}

/// Trait for implementing Wasm Memory used by Wasmer.
pub trait Memory: fmt::Debug + Send + Sync {
    /// Returns the memory type for this memory.
    fn ty(&self) -> &MemoryType;

    /// Returns the memory style for this memory.
    fn style(&self) -> &MemoryStyle;

    /// Returns the number of allocated wasm pages.
    fn size(&self) -> Pages;

    /// Grow memory by the specified amount of wasm pages.
    fn grow(&self, delta: Pages) -> Result<Pages, MemoryError>;

    /// Return a [`VMMemoryDefinition`] for exposing the memory to compiled wasm code.
    ///
    /// The pointer returned in [`VMMemoryDefinition`] must be valid for the lifetime of this memory.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition>;
}

/// A linear memory instance.
#[derive(Debug)]
pub struct LinearMemory {
    // The underlying allocation.
    mmap: Mutex<WasmMmap>,

    // The optional maximum size in wasm pages of this linear memory.
    maximum: Option<Pages>,

    /// The WebAssembly linear memory description.
    memory: MemoryType,

    /// Our chosen implementation style.
    style: MemoryStyle,

    // Size in bytes of extra guard pages after the end to optimize loads and stores with
    // constant offsets.
    offset_guard_size: usize,

    /// The owned memory definition used by the generated code
    vm_memory_definition: Box<UnsafeCell<VMMemoryDefinition>>,

    // Records whether we're using a bounds-checking strategy which requires
    // handlers to catch trapping accesses.
    pub(crate) needs_signal_handlers: bool,
}

/// This is correct because all internal mutability is protected by a mutex.
unsafe impl Sync for LinearMemory {}

#[derive(Debug)]
struct WasmMmap {
    // Our OS allocation of mmap'd memory.
    alloc: Mmap,
    // The current logical size in wasm pages of this linear memory.
    size: Pages,
}

impl LinearMemory {
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    pub fn new(memory: &MemoryType, style: &MemoryStyle) -> Result<Self, MemoryError> {
        // `maximum` cannot be set to more than `65536` pages.
        assert_le!(memory.minimum, Pages::max_value());
        assert!(memory.maximum.is_none() || memory.maximum.unwrap() <= Pages::max_value());

        if memory.maximum.is_some() && memory.maximum.unwrap() < memory.minimum {
            return Err(MemoryError::InvalidMemory {
                reason: format!(
                    "the maximum ({} pages) is less than the minimum ({} pages)",
                    memory.maximum.unwrap().0,
                    memory.minimum.0
                ),
            });
        }

        let offset_guard_bytes = style.offset_guard_size() as usize;

        // If we have an offset guard, or if we're doing the static memory
        // allocation strategy, we need signal handlers to catch out of bounds
        // acceses.
        let needs_signal_handlers = offset_guard_bytes > 0
            || match style {
                MemoryStyle::Dynamic { .. } => false,
                MemoryStyle::Static { .. } => true,
            };

        let minimum_pages = match style {
            MemoryStyle::Dynamic { .. } => memory.minimum,
            MemoryStyle::Static { bound, .. } => {
                assert_ge!(*bound, memory.minimum);
                *bound
            }
        };
        let minimum_bytes = minimum_pages.bytes().0;
        let request_bytes = minimum_bytes.checked_add(offset_guard_bytes).unwrap();
        let mapped_pages = memory.minimum;
        let mapped_bytes = mapped_pages.bytes();

        let mut mmap = WasmMmap {
            alloc: Mmap::accessible_reserved(mapped_bytes.0, request_bytes)
                .map_err(MemoryError::Region)?,
            size: memory.minimum,
        };

        let base_ptr = mmap.alloc.as_mut_ptr();
        Ok(Self {
            mmap: Mutex::new(mmap),
            maximum: memory.maximum,
            offset_guard_size: offset_guard_bytes,
            needs_signal_handlers,
            vm_memory_definition: Box::new(UnsafeCell::new(VMMemoryDefinition {
                base: base_ptr,
                current_length: memory.minimum.bytes().0.try_into().unwrap(),
            })),
            memory: memory.clone(),
            style: style.clone(),
        })
    }
}

impl Memory for LinearMemory {
    /// Returns the type for this memory.
    fn ty(&self) -> &MemoryType {
        &self.memory
    }

    /// Returns the memory style for this memory.
    fn style(&self) -> &MemoryStyle {
        &self.style
    }

    /// Returns the number of allocated wasm pages.
    fn size(&self) -> Pages {
        unsafe {
            let ptr = self.vm_memory_definition.get();
            Bytes::from((*ptr).current_length).into()
        }
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&self, delta: Pages) -> Result<Pages, MemoryError> {
        let mut mmap_guard = self.mmap.lock().unwrap();
        let mmap = mmap_guard.borrow_mut();
        // Optimization of memory.grow 0 calls.
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
        // update memory definition
        unsafe {
            let md = &mut *self.vm_memory_definition.get();
            md.current_length = new_pages.bytes().0.try_into().unwrap();
            md.base = mmap.alloc.as_mut_ptr() as _;
        }

        Ok(prev_pages)
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        let _mmap_guard = self.mmap.lock().unwrap();
        let ptr = self.vm_memory_definition.as_ref() as *const UnsafeCell<VMMemoryDefinition>
            as *const VMMemoryDefinition as *mut VMMemoryDefinition;
        unsafe { NonNull::new_unchecked(ptr) }
    }
}
