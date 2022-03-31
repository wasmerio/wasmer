// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! Memory management for linear memories.
//!
//! `LinearMemory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use crate::vmcontext::VMMemoryDefinition;
use loupe::MemoryUsage;
use std::cell::UnsafeCell;
use std::convert::TryInto;
use std::fmt;
use std::ptr::NonNull;
use std::sync::Mutex;
use wasmer_types::{Bytes, MemoryError, MemoryStyle, MemoryType, Pages};

/// Trait for implementing Wasm Memory used by Wasmer.
pub trait Memory: fmt::Debug + Send + Sync + MemoryUsage {
    /// Returns the memory type for this memory.
    fn ty(&self) -> MemoryType;

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
#[derive(Debug, MemoryUsage)]
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
    vm_memory_definition: VMMemoryDefinitionOwnership,

    // Records whether we're using a bounds-checking strategy which requires
    // handlers to catch trapping accesses.
    pub(crate) needs_signal_handlers: bool,
}

/// A type to help manage who is responsible for the backing memory of them
/// `VMMemoryDefinition`.
#[allow(dead_code)]
#[derive(Debug, MemoryUsage)]
enum VMMemoryDefinitionOwnership {
    /// The `VMMemoryDefinition` is owned by the `Instance` and we should use
    /// its memory. This is how a local memory that's exported should be stored.
    VMOwned(NonNull<VMMemoryDefinition>),
    /// The `VMMemoryDefinition` is owned by the host and we should manage its
    /// memory. This is how an imported memory that doesn't come from another
    /// Wasm module should be stored.
    HostOwned(Box<UnsafeCell<VMMemoryDefinition>>),
}

/// We must implement this because of `VMMemoryDefinitionOwnership::VMOwned`.
/// This is correct because synchronization of memory accesses is controlled
/// by the VM.
// REVIEW: I don't believe ^; this probably shouldn't be `Send`...
// mutations from other threads into this data could be a problem, but we probably
// don't want to use atomics for this in the generated code.
// TODO:
unsafe impl Send for LinearMemory {}

/// This is correct because all internal mutability is protected by a mutex.
unsafe impl Sync for LinearMemory {}

#[derive(Debug, MemoryUsage)]
struct WasmMmap {
    // Our OS allocation of mmap'd memory.
    alloc: usize,
    // The current logical size in wasm pages of this linear memory.
    size: Pages,
}

impl LinearMemory {
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `LinearMemory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new(memory: &MemoryType, style: &MemoryStyle) -> Result<Self, MemoryError> {
        unsafe { Self::new_internal(memory, style, None) }
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `LinearMemory` with metadata owned by a VM, pointed to by
    /// `vm_memory_location`: this can be used to create a local memory.
    ///
    /// # Safety
    /// - `vm_memory_location` must point to a valid location in VM memory.
    pub unsafe fn from_definition(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: NonNull<VMMemoryDefinition>,
    ) -> Result<Self, MemoryError> {
        Self::new_internal(memory, style, Some(vm_memory_location))
    }

    /// Build a `LinearMemory` with either self-owned or VM owned metadata.
    unsafe fn new_internal(
        _memory: &MemoryType,
        _style: &MemoryStyle,
        _vm_memory_location: Option<NonNull<VMMemoryDefinition>>,
    ) -> Result<Self, MemoryError> {
        Err(MemoryError::Generic("Not implemented".to_string()))
    }

    /// Get the `VMMemoryDefinition`.
    ///
    /// # Safety
    /// - You must ensure that you have mutually exclusive access before calling
    ///   this function. You can get this by locking the `mmap` mutex.
    unsafe fn get_vm_memory_definition(&self) -> NonNull<VMMemoryDefinition> {
        match &self.vm_memory_definition {
            VMMemoryDefinitionOwnership::VMOwned(ptr) => *ptr,
            VMMemoryDefinitionOwnership::HostOwned(boxed_ptr) => {
                NonNull::new_unchecked(boxed_ptr.get())
            }
        }
    }
}

impl Memory for LinearMemory {
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        let minimum = self.size();
        let mut out = self.memory.clone();
        out.minimum = minimum;

        out
    }

    /// Returns the memory style for this memory.
    fn style(&self) -> &MemoryStyle {
        &self.style
    }

    /// Returns the number of allocated wasm pages.
    fn size(&self) -> Pages {
        // TODO: investigate this function for race conditions
        unsafe {
            let md_ptr = self.get_vm_memory_definition();
            let md = md_ptr.as_ref();
            Bytes::from(md.current_length).try_into().unwrap()
        }
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&self, _delta: Pages) -> Result<Pages, MemoryError> {
        Err(MemoryError::Generic("Not implemented".to_string()))
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        let _mmap_guard = self.mmap.lock().unwrap();
        unsafe { self.get_vm_memory_definition() }
    }
}
