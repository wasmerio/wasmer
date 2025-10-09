//! Memory management for linear memories.
//!
//! `Memory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.
// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

use std::{cell::UnsafeCell, convert::TryInto, ptr::NonNull, rc::Rc, sync::RwLock};

use wasmer::{Bytes, MemoryError, MemoryType, Pages};
use wasmer_types::{MemoryStyle, WASM_PAGE_SIZE};
use wasmer_vm::{
    LinearMemory, MaybeInstanceOwned, ThreadConditions, Trap, VMMemoryDefinition, WaiterError,
};

use super::fd_mmap::FdMmap;

// use crate::trap::Trap;
// use crate::{mmap::Mmap, store::MaybeInstanceOwned, vmcontext::VMMemoryDefinition};
// use more_asserts::assert_ge;
// use std::cell::UnsafeCell;
// use std::convert::TryInto;
// use std::ptr::NonNull;
// use std::slice;
// use std::sync::{Arc, RwLock};
// use wasmer_types::{Bytes, MemoryError, MemoryStyle, MemoryType, Pages};

// The memory mapped area
#[derive(Debug)]
struct WasmMmap {
    // Our OS allocation of mmap'd memory.
    alloc: FdMmap,
    // The current logical size in wasm pages of this linear memory.
    size: Pages,
    /// The owned memory definition used by the generated code
    vm_memory_definition: MaybeInstanceOwned<VMMemoryDefinition>,
}

impl WasmMmap {
    fn get_vm_memory_definition(&self) -> NonNull<VMMemoryDefinition> {
        self.vm_memory_definition.as_ptr()
    }

    fn size(&self) -> Pages {
        unsafe {
            let md_ptr = self.get_vm_memory_definition();
            let md = md_ptr.as_ref();
            Bytes::from(md.current_length).try_into().unwrap()
        }
    }

    fn grow(&mut self, delta: Pages, conf: VMMemoryConfig) -> Result<Pages, MemoryError> {
        // Optimization of memory.grow 0 calls.
        if delta.0 == 0 {
            return Ok(self.size);
        }

        let new_pages = self
            .size
            .checked_add(delta)
            .ok_or(MemoryError::CouldNotGrow {
                current: self.size,
                attempted_delta: delta,
            })?;
        let prev_pages = self.size;

        if let Some(maximum) = conf.maximum {
            if new_pages > maximum {
                return Err(MemoryError::CouldNotGrow {
                    current: self.size,
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
                current: self.size,
                attempted_delta: delta,
            });
        }

        let delta_bytes = delta.bytes().0;
        let prev_bytes = prev_pages.bytes().0;
        let new_bytes = new_pages.bytes().0;

        if new_bytes > self.alloc.len() - conf.offset_guard_size {
            // If the new size is within the declared maximum, but needs more memory than we
            // have on hand, it's a dynamic heap and it can move.
            let guard_bytes = conf.offset_guard_size;
            let request_bytes =
                new_bytes
                    .checked_add(guard_bytes)
                    .ok_or_else(|| MemoryError::CouldNotGrow {
                        current: new_pages,
                        attempted_delta: Bytes(guard_bytes).try_into().unwrap(),
                    })?;

            let mut new_mmap = FdMmap::accessible_reserved(new_bytes, request_bytes)
                .map_err(MemoryError::Region)?;

            let copy_len = self.alloc.len() - conf.offset_guard_size;
            new_mmap.as_mut_slice()[..copy_len].copy_from_slice(&self.alloc.as_slice()[..copy_len]);

            self.alloc = new_mmap;
        } else if delta_bytes > 0 {
            // Make the newly allocated pages accessible.
            self.alloc
                .make_accessible(prev_bytes, delta_bytes)
                .map_err(MemoryError::Region)?;
        }

        self.size = new_pages;

        // update memory definition
        unsafe {
            let mut md_ptr = self.vm_memory_definition.as_ptr();
            let md = md_ptr.as_mut();
            md.current_length = new_pages.bytes().0;
            md.base = self.alloc.as_mut_ptr() as _;
        }

        Ok(prev_pages)
    }

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    fn grow_at_least(&mut self, min_size: u64, conf: VMMemoryConfig) -> Result<(), MemoryError> {
        let cur_size = self.size.bytes().0 as u64;
        if cur_size < min_size {
            let growth = min_size - cur_size;
            let growth_pages = ((growth - 1) / WASM_PAGE_SIZE as u64) + 1;
            self.grow(Pages(growth_pages as u32), conf)?;
        }

        Ok(())
    }

    fn reset(&mut self) -> Result<(), MemoryError> {
        self.size.0 = 0;
        Ok(())
    }

    /// Copies the memory
    /// (in this case it performs a copy-on-write to save memory)
    pub fn copy(&mut self) -> Result<Self, MemoryError> {
        let mem_length = self.size.bytes().0;
        let mut alloc = self
            .alloc
            .duplicate(Some(mem_length))
            .map_err(MemoryError::Generic)?;
        let base_ptr = alloc.as_mut_ptr();
        Ok(Self {
            vm_memory_definition: MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(
                VMMemoryDefinition {
                    base: base_ptr,
                    current_length: mem_length,
                },
            ))),
            alloc,
            size: self.size,
        })
    }
}

/// A linear memory instance.
#[derive(Debug, Clone)]
struct VMMemoryConfig {
    // The optional maximum size in wasm pages of this linear memory.
    maximum: Option<Pages>,
    /// The WebAssembly linear memory description.
    memory: MemoryType,
    /// Our chosen implementation style.
    style: MemoryStyle,
    // Size in bytes of extra guard pages after the end to optimize loads and stores with
    // constant offsets.
    offset_guard_size: usize,
}

impl VMMemoryConfig {
    fn ty(&self, minimum: Pages) -> MemoryType {
        let mut out = self.memory;
        out.minimum = minimum;

        out
    }

    fn style(&self) -> MemoryStyle {
        self.style
    }
}

/// A linear memory instance.
#[derive(Debug)]
pub struct VMOwnedMemory {
    // The underlying allocation.
    mmap: WasmMmap,
    // Configuration of this memory
    config: VMMemoryConfig,
}

unsafe impl Send for VMOwnedMemory {}
unsafe impl Sync for VMOwnedMemory {}

impl VMOwnedMemory {
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new(memory: &MemoryType, style: &MemoryStyle) -> Result<Self, MemoryError> {
        unsafe { Self::new_internal(memory, style, None) }
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with metadata owned by a VM, pointed to by
    /// `vm_memory_location`: this can be used to create a local memory.
    ///
    /// # Safety
    /// - `vm_memory_location` must point to a valid location in VM memory.
    pub unsafe fn from_definition(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: NonNull<VMMemoryDefinition>,
    ) -> Result<Self, MemoryError> {
        unsafe { Self::new_internal(memory, style, Some(vm_memory_location)) }
    }

    /// Build a `Memory` with either self-owned or VM owned metadata.
    unsafe fn new_internal(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: Option<NonNull<VMMemoryDefinition>>,
    ) -> Result<Self, MemoryError> {
        if memory.minimum > Pages::max_value() {
            return Err(MemoryError::MinimumMemoryTooLarge {
                min_requested: memory.minimum,
                max_allowed: Pages::max_value(),
            });
        }
        // `maximum` cannot be set to more than `65536` pages.
        if let Some(max) = memory.maximum {
            if max > Pages::max_value() {
                return Err(MemoryError::MaximumMemoryTooLarge {
                    max_requested: max,
                    max_allowed: Pages::max_value(),
                });
            }
            if max < memory.minimum {
                return Err(MemoryError::InvalidMemory {
                    reason: format!(
                        "the maximum ({} pages) is less than the minimum ({} pages)",
                        max.0, memory.minimum.0
                    ),
                });
            }
        }

        let offset_guard_bytes = style.offset_guard_size() as usize;

        let minimum_pages = match style {
            MemoryStyle::Dynamic { .. } => memory.minimum,
            MemoryStyle::Static { bound, .. } => {
                assert!(*bound >= memory.minimum);
                *bound
            }
        };
        let minimum_bytes = minimum_pages.bytes().0;
        let request_bytes = minimum_bytes.checked_add(offset_guard_bytes).unwrap();
        let mapped_pages = memory.minimum;
        let mapped_bytes = mapped_pages.bytes();

        let mut alloc = FdMmap::accessible_reserved(mapped_bytes.0, request_bytes)
            .map_err(MemoryError::Region)?;
        let base_ptr = alloc.as_mut_ptr();
        let mem_length = memory.minimum.bytes().0;
        let mmap = WasmMmap {
            vm_memory_definition: if let Some(mem_loc) = vm_memory_location {
                {
                    let mut ptr = mem_loc;
                    let md = unsafe { ptr.as_mut() };
                    md.base = base_ptr;
                    md.current_length = mem_length;
                }
                MaybeInstanceOwned::Instance(mem_loc)
            } else {
                MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(VMMemoryDefinition {
                    base: base_ptr,
                    current_length: mem_length,
                })))
            },
            alloc,
            size: memory.minimum,
        };

        Ok(Self {
            mmap,
            config: VMMemoryConfig {
                maximum: memory.maximum,
                offset_guard_size: offset_guard_bytes,
                memory: *memory,
                style: *style,
            },
        })
    }

    /// Converts this owned memory into shared memory
    pub fn to_shared(self) -> VMSharedMemory {
        VMSharedMemory {
            mmap: Rc::new(RwLock::new(self.mmap)),
            config: self.config,
            conditions: ThreadConditions::new(),
        }
    }

    /// Copies this memory to a new memory
    pub fn copy(&mut self) -> Result<Self, MemoryError> {
        Ok(Self {
            mmap: self.mmap.copy()?,
            config: self.config.clone(),
        })
    }
}

impl LinearMemory for VMOwnedMemory {
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        let minimum = self.mmap.size();
        self.config.ty(minimum)
    }

    /// Returns the size of hte memory in pages
    fn size(&self) -> Pages {
        self.mmap.size()
    }

    /// Returns the memory style for this memory.
    fn style(&self) -> MemoryStyle {
        self.config.style()
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
        self.mmap.grow(delta, self.config.clone())
    }

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    fn grow_at_least(&mut self, min_size: u64) -> Result<(), MemoryError> {
        self.mmap.grow_at_least(min_size, self.config.clone())
    }

    fn reset(&mut self) -> Result<(), MemoryError> {
        self.mmap.reset()?;
        Ok(())
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        self.mmap.vm_memory_definition.as_ptr()
    }

    /// Owned memory can not be cloned (this will always return None)
    fn try_clone(&self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        Err(MemoryError::MemoryNotShared)
    }

    /// Copies this memory to a new memory
    fn copy(&mut self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        let forked = Self::copy(self)?;
        Ok(Box::new(forked))
    }
}

/// A shared linear memory instance.
#[derive(Debug, Clone)]
pub struct VMSharedMemory {
    // The underlying allocation.
    mmap: Rc<RwLock<WasmMmap>>,
    // Configuration of this memory
    config: VMMemoryConfig,
    conditions: ThreadConditions,
}

unsafe impl Send for VMSharedMemory {}
unsafe impl Sync for VMSharedMemory {}

impl VMSharedMemory {
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new(memory: &MemoryType, style: &MemoryStyle) -> Result<Self, MemoryError> {
        Ok(VMOwnedMemory::new(memory, style)?.to_shared())
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with metadata owned by a VM, pointed to by
    /// `vm_memory_location`: this can be used to create a local memory.
    ///
    /// # Safety
    /// - `vm_memory_location` must point to a valid location in VM memory.
    pub unsafe fn from_definition(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: NonNull<VMMemoryDefinition>,
    ) -> Result<Self, MemoryError> {
        let owned = unsafe { VMOwnedMemory::from_definition(memory, style, vm_memory_location)? };
        Ok(owned.to_shared())
    }

    /// Copies this memory to a new memory
    pub fn copy(&mut self) -> Result<Self, MemoryError> {
        let mut guard = self.mmap.write().unwrap();
        Ok(Self {
            mmap: Rc::new(RwLock::new(guard.copy()?)),
            config: self.config.clone(),
            conditions: ThreadConditions::new(),
        })
    }
}

impl LinearMemory for VMSharedMemory {
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        let minimum = {
            let guard = self.mmap.read().unwrap();
            guard.size()
        };
        self.config.ty(minimum)
    }

    /// Returns the size of hte memory in pages
    fn size(&self) -> Pages {
        let guard = self.mmap.read().unwrap();
        guard.size()
    }

    /// Resets the memory back down to zero size
    fn reset(&mut self) -> Result<(), MemoryError> {
        let mut guard = self.mmap.write().unwrap();
        guard.reset()?;
        Ok(())
    }

    /// Returns the memory style for this memory.
    fn style(&self) -> MemoryStyle {
        self.config.style()
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
        let mut guard = self.mmap.write().unwrap();
        guard.grow(delta, self.config.clone())
    }

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    fn grow_at_least(&mut self, min_size: u64) -> Result<(), MemoryError> {
        let mut guard = self.mmap.write().unwrap();
        guard.grow_at_least(min_size, self.config.clone())
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        let guard = self.mmap.read().unwrap();
        guard.vm_memory_definition.as_ptr()
    }

    /// Shared memory can always be cloned
    fn try_clone(&self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        Ok(Box::new(self.clone()))
    }

    /// Copies this memory to a new memory
    fn copy(&mut self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        let forked = Self::copy(self)?;
        Ok(Box::new(forked))
    }

    fn do_wait(
        &mut self,
        dst: wasmer_vm::NotifyLocation,
        timeout: Option<std::time::Duration>,
    ) -> Result<u32, WaiterError> {
        self.conditions.do_wait(dst, timeout)
    }

    fn do_notify(&mut self, dst: wasmer_vm::NotifyLocation, count: u32) -> u32 {
        self.conditions.do_notify(dst, count)
    }
}

impl From<VMOwnedMemory> for VMMemory {
    fn from(mem: VMOwnedMemory) -> Self {
        Self(Box::new(mem))
    }
}

impl From<VMSharedMemory> for VMMemory {
    fn from(mem: VMSharedMemory) -> Self {
        Self(Box::new(mem))
    }
}

/// Represents linear memory that can be either owned or shared
#[derive(Debug)]
pub struct VMMemory(pub Box<dyn LinearMemory + 'static>);

impl From<Box<dyn LinearMemory + 'static>> for VMMemory {
    fn from(mem: Box<dyn LinearMemory + 'static>) -> Self {
        Self(mem)
    }
}

impl LinearMemory for VMMemory {
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        self.0.ty()
    }

    /// Returns the size of hte memory in pages
    fn size(&self) -> Pages {
        self.0.size()
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
        self.0.grow(delta)
    }

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    fn grow_at_least(&mut self, min_size: u64) -> Result<(), MemoryError> {
        self.0.grow_at_least(min_size)
    }

    /// Resets the memory down to a zero size
    fn reset(&mut self) -> Result<(), MemoryError> {
        self.0.reset()?;
        Ok(())
    }

    /// Returns the memory style for this memory.
    fn style(&self) -> MemoryStyle {
        self.0.style()
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        self.0.vmmemory()
    }

    /// Attempts to clone this memory (if its clonable)
    fn try_clone(&self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        self.0.try_clone()
    }

    /// Initialize memory with data
    unsafe fn initialize_with_data(&self, start: usize, data: &[u8]) -> Result<(), Trap> {
        unsafe { self.0.initialize_with_data(start, data) }
    }

    /// Copies this memory to a new memory
    fn copy(&mut self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        self.0.copy()
    }
}

impl VMMemory {
    /// Creates a new linear memory instance of the correct type with specified
    /// minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new(memory: &MemoryType, style: &MemoryStyle) -> Result<Self, MemoryError> {
        Ok(if memory.shared {
            Self(Box::new(VMSharedMemory::new(memory, style)?))
        } else {
            Self(Box::new(VMOwnedMemory::new(memory, style)?))
        })
    }

    /// Returns the number of pages in the allocated memory block
    pub fn get_runtime_size(&self) -> u32 {
        self.0.size().0
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with metadata owned by a VM, pointed to by
    /// `vm_memory_location`: this can be used to create a local memory.
    ///
    /// # Safety
    /// - `vm_memory_location` must point to a valid location in VM memory.
    pub unsafe fn from_definition(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: NonNull<VMMemoryDefinition>,
    ) -> Result<Self, MemoryError> {
        Ok(if memory.shared {
            let shared =
                unsafe { VMSharedMemory::from_definition(memory, style, vm_memory_location)? };
            Self(Box::new(shared))
        } else {
            let owned =
                unsafe { VMOwnedMemory::from_definition(memory, style, vm_memory_location)? };
            Self(Box::new(owned))
        })
    }

    /// Creates VMMemory from a custom implementation - the following into implementations
    /// are natively supported
    /// - VMOwnedMemory -> VMMemory
    /// - Box<dyn LinearMemory + 'static> -> VMMemory
    pub fn from_custom<IntoVMMemory>(memory: IntoVMMemory) -> Self
    where
        IntoVMMemory: Into<Self>,
    {
        memory.into()
    }

    /// Copies this memory to a new memory
    pub fn copy(&mut self) -> Result<Box<dyn LinearMemory + 'static>, MemoryError> {
        LinearMemory::copy(self)
    }
}

#[doc(hidden)]
/// Default implementation to initialize memory with data
pub unsafe fn initialize_memory_with_data(
    memory: &VMMemoryDefinition,
    start: usize,
    data: &[u8],
) -> Result<(), Trap> {
    let mem_slice = unsafe { std::slice::from_raw_parts_mut(memory.base, memory.current_length) };
    let end = start + data.len();
    let to_init = &mut mem_slice[start..end];
    to_init.copy_from_slice(data);

    Ok(())
}
