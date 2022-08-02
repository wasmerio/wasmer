// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

//! Memory management for linear memories.
//!
//! `Memory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use crate::{mmap::Mmap, store::MaybeInstanceOwned};
use more_asserts::assert_ge;
use std::cell::UnsafeCell;
use std::convert::TryInto;
use std::ptr::NonNull;
use std::sync::{RwLock, Arc};
use wasmer_types::{Bytes, MemoryStyle, MemoryType, Pages, MemoryError, LinearMemory, LinearMemoryDefinition};

// The memory mapped area
#[derive(Debug)]
struct WasmMmap {
    // Our OS allocation of mmap'd memory.
    alloc: Mmap,
    // The current logical size in wasm pages of this linear memory.
    size: Pages,
    /// The owned memory definition used by the generated code
    vm_memory_definition: MaybeInstanceOwned<LinearMemoryDefinition>,
}

impl WasmMmap
{
    fn get_vm_memory_definition(&self) -> NonNull<LinearMemoryDefinition> {
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

            let mut new_mmap =
                Mmap::accessible_reserved(new_bytes, request_bytes).map_err(MemoryError::Region)?;

            let copy_len = self.alloc.len() - conf.offset_guard_size;
            new_mmap.as_mut_slice()[..copy_len]
                .copy_from_slice(&self.alloc.as_slice()[..copy_len]);

            self.alloc = new_mmap;
        } else if delta_bytes > 0 {
            // Make the newly allocated pages accessible.
            self
                .alloc
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
}

pub(crate) mod ops {
    use std::ptr;
    use std::convert::TryFrom;

    use wasmer_types::{LinearMemoryDefinition, TrapCode};

    use crate::Trap;

    /// Do an unsynchronized, non-atomic `memory.copy` for the memory.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error when the source or destination ranges are out of
    /// bounds.
    ///
    /// # Safety
    /// The memory is not copied atomically and is not synchronized: it's the
    /// caller's responsibility to synchronize.
    pub(crate) unsafe fn memory_copy(mem: &LinearMemoryDefinition, dst: u32, src: u32, len: u32) -> Result<(), Trap> {
        // https://webassembly.github.io/reference-types/core/exec/instructions.html#exec-memory-copy
        if src
            .checked_add(len)
            .map_or(true, |n| usize::try_from(n).unwrap() > mem.current_length)
            || dst
                .checked_add(len)
                .map_or(true, |m| usize::try_from(m).unwrap() > mem.current_length)
        {
            return Err(Trap::lib(TrapCode::HeapAccessOutOfBounds));
        }

        let dst = usize::try_from(dst).unwrap();
        let src = usize::try_from(src).unwrap();

        // Bounds and casts are checked above, by this point we know that
        // everything is safe.
        let dst = mem.base.add(dst);
        let src = mem.base.add(src);
        ptr::copy(src, dst, len as usize);

        Ok(())
    }

    /// Perform the `memory.fill` operation for the memory in an unsynchronized,
    /// non-atomic way.
    ///
    /// # Errors
    ///
    /// Returns a `Trap` error if the memory range is out of bounds.
    ///
    /// # Safety
    /// The memory is not filled atomically and is not synchronized: it's the
    /// caller's responsibility to synchronize.
    pub(crate) unsafe fn memory_fill(mem: &LinearMemoryDefinition, dst: u32, val: u32, len: u32) -> Result<(), Trap> {
        if dst
            .checked_add(len)
            .map_or(true, |m| usize::try_from(m).unwrap() > mem.current_length)
        {
            return Err(Trap::lib(TrapCode::HeapAccessOutOfBounds));
        }

        let dst = isize::try_from(dst).unwrap();
        let val = val as u8;

        // Bounds and casts are checked above, by this point we know that
        // everything is safe.
        let dst = mem.base.offset(dst);
        ptr::write_bytes(dst, val, len as usize);

        Ok(())
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

impl VMMemoryConfig
{
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

/// A shared linear memory instance.
#[derive(Debug, Clone)]
pub struct VMSharedMemory {
    // The underlying allocation.
    mmap: Arc<RwLock<WasmMmap>>,
    // Configuration of this memory
    config: VMMemoryConfig,
}

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
        vm_memory_location: NonNull<LinearMemoryDefinition>,
    ) -> Result<Self, MemoryError> {
        Self::new_internal(memory, style, Some(vm_memory_location))
    }

    /// Build a `Memory` with either self-owned or VM owned metadata.
    unsafe fn new_internal(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: Option<NonNull<LinearMemoryDefinition>>,
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
                assert_ge!(*bound, memory.minimum);
                *bound
            }
        };
        let minimum_bytes = minimum_pages.bytes().0;
        let request_bytes = minimum_bytes.checked_add(offset_guard_bytes).unwrap();
        let mapped_pages = memory.minimum;
        let mapped_bytes = mapped_pages.bytes();

        let mut alloc = Mmap::accessible_reserved(mapped_bytes.0, request_bytes)
            .map_err(MemoryError::Region)?;
        let base_ptr = alloc.as_mut_ptr();
        let mem_length = memory.minimum.bytes().0;
        let mmap = WasmMmap {
            vm_memory_definition: if let Some(mem_loc) = vm_memory_location {
                {
                    let mut ptr = mem_loc;
                    let md = ptr.as_mut();
                    md.base = base_ptr;
                    md.current_length = mem_length;
                }
                MaybeInstanceOwned::Instance(mem_loc)
            } else {
                MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(LinearMemoryDefinition {
                    base: base_ptr,
                    current_length: mem_length,
                })))
            },
            alloc,
            size: memory.minimum,
        };
        
        Ok(Self {
            mmap: mmap,
            config: VMMemoryConfig {
                maximum: memory.maximum,
                offset_guard_size: offset_guard_bytes,
                memory: *memory,
                style: style.clone(),
            }
        })
    }
}

impl VMOwnedMemory
{
    /// Converts this owned memory into shared memory
    pub fn to_shared(self) -> VMSharedMemory
    {
        VMSharedMemory {
            mmap: Arc::new(RwLock::new(self.mmap)),
            config: self.config
        }
    }

    /// Returns the memory style for this memory.
    pub fn style(&self) -> MemoryStyle {
        self.config.style()
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    pub fn as_ptr(&self) -> NonNull<LinearMemoryDefinition> {
        self.mmap.vm_memory_definition.as_ptr()
    }
}

impl LinearMemory
for VMOwnedMemory
{
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        let minimum = self.mmap.size();
        self.config.ty(minimum)
    }

    /// Returns the size of hte memory in pages
    fn size(&self) -> Pages {
        self.mmap.size()
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
        self.mmap.grow(delta, self.config.clone())
    }
}

impl VMSharedMemory
{
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new(memory: &MemoryType, style: &MemoryStyle) -> Result<Self, MemoryError> {
        Ok(
            VMOwnedMemory::new(memory, style)?.to_shared()
        )
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
        vm_memory_location: NonNull<LinearMemoryDefinition>,
    ) -> Result<Self, MemoryError> {
        Ok(
            VMOwnedMemory::from_definition(memory, style, vm_memory_location)?.to_shared()
        )
    }

    /// Returns the memory style for this memory.
    pub fn style(&self) -> MemoryStyle {
        self.config.style()
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    pub fn as_ptr(&self) -> NonNull<LinearMemoryDefinition> {
        let guard = self.mmap.read().unwrap();
        guard.vm_memory_definition.as_ptr()
    }
}

impl LinearMemory
for VMSharedMemory
{
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

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
        let mut guard = self.mmap.write().unwrap();
        guard.grow(delta, self.config.clone())
    }
}

/// Represents linear memory that can be either owned or shared
#[derive(Debug)]
pub enum VMMemory
{
    /// The memory is owned by a thread and can not be shared
    /// which means it does NOT support multithreading
    Owned(VMOwnedMemory),
    /// Memory that can be cloned and thus supports multithreading
    /// however it has a higher operational cost
    Shared(VMSharedMemory)
}

// VMMemoryMmap is protected by a RwLock and Arc referencing counting
// however it keeps a pointer to the last known state of the Mmap
// for performance reasons - this means that the memory pointer must
// always remain valid - essentially these rules
// - it grows but never shrinks
// - the base pointer never moves   
unsafe impl Send for VMMemory { }
unsafe impl Sync for VMMemory { }

impl LinearMemory
for VMMemory
{
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        match self {
            Self::Owned(m) => m.ty(),
            Self::Shared(m) => m.ty(),
        }
    }

    /// Returns the size of hte memory in pages
    fn size(&self) -> Pages {
        match self {
            Self::Owned(m) => m.size(),
            Self::Shared(m) => m.size(),
        }
    }

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError> {
        match self {
            Self::Owned(m) => m.grow(delta),
            Self::Shared(m) => m.grow(delta),
        }
    }
}

impl VMMemory
{
    /// Returns the memory style for this memory.
    pub fn style(&self) -> MemoryStyle {
        match self {
            Self::Owned(m) => m.style(),
            Self::Shared(m) => m.style(),
        }
    }

    /// Attempts to clone this memory (if its clonable)
    pub fn try_clone(&self) -> Option<VMMemory> {
        match self {
            Self::Owned(_) => None,
            Self::Shared(m) => Some(VMMemory::Shared(m.clone())),
        }
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    pub fn as_ptr(&self) -> NonNull<LinearMemoryDefinition> {
        match self {
            Self::Owned(m) => m.as_ptr(),
            Self::Shared(m) => m.as_ptr(),
        }
    }
}

/// Creates a new linear memory instance of the correct type with specified
/// minimum and maximum number of wasm pages.
///
/// This creates a `Memory` with owned metadata: this can be used to create a memory
/// that will be imported into Wasm modules.
pub fn create_memory(memory: &MemoryType, style: &MemoryStyle) -> Result<VMMemory, MemoryError> {
    Ok(
        if memory.shared {
            VMMemory::Shared(VMSharedMemory::new(memory, style)?)
        } else {
            VMMemory::Owned(VMOwnedMemory::new(memory, style)?)
        }
    )
}

/// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
///
/// This creates a `Memory` with metadata owned by a VM, pointed to by
/// `vm_memory_location`: this can be used to create a local memory.
///
/// # Safety
/// - `vm_memory_location` must point to a valid location in VM memory.
pub unsafe fn create_memory_from_definition(
    memory: &MemoryType,
    style: &MemoryStyle,
    vm_memory_location: NonNull<LinearMemoryDefinition>,
) -> Result<VMMemory, MemoryError> {
    Ok(
        if memory.shared {
            VMMemory::Shared(VMSharedMemory::from_definition(memory, style, vm_memory_location)?)
        } else {
            VMMemory::Owned(VMOwnedMemory::from_definition(memory, style, vm_memory_location)?)
        }
    )
}
