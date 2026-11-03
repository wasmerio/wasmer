// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Memory management for linear memories.
//!
//! `Memory` is to WebAssembly linear memories what `Table` is to WebAssembly tables.

use crate::threadconditions::ThreadConditions;
pub use crate::threadconditions::{NotifyLocation, WaiterError};
use crate::trap::Trap;
use crate::{
    mmap::{Mmap, MmapType},
    store::MaybeInstanceOwned,
    threadconditions::ExpectedValue,
    vmcontext::VMMemoryDefinition,
};
use more_asserts::assert_ge;
use std::cell::UnsafeCell;
use std::convert::TryInto;
use std::ptr::NonNull;
use std::slice;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use wasmer_types::{Bytes, MemoryError, MemoryStyle, MemoryType, Pages, WASM_PAGE_SIZE};

// The memory mapped area
#[derive(Debug)]
struct WasmMmap {
    // Our OS allocation of mmap'd memory.
    alloc: Mmap,
    // The current logical size in wasm pages of this linear memory.
    size: Pages,
    /// The owned memory definition used by the generated code
    vm_memory_definition: MaybeInstanceOwned<VMMemoryDefinition>,
}

/// # SAFETY: Not safe by rust standards, since guest code may do weird things
/// with its memory. However, this is still safe to send across threads as
/// far as the WASM spec is concerned.
unsafe impl Send for WasmMmap {}
/// # SAFETY: see above.
unsafe impl Sync for WasmMmap {}

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

        if let Some(maximum) = conf.maximum
            && new_pages > maximum
        {
            return Err(MemoryError::CouldNotGrow {
                current: self.size,
                attempted_delta: delta,
            });
        }

        // Wasm linear memories are never allowed to grow beyond what is
        // indexable. If the memory has no maximum, enforce the greatest
        // limit here.
        if new_pages > Pages::max_value() {
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
                Mmap::accessible_reserved(new_bytes, request_bytes, None, MmapType::Private, true)
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

    /// Resets the memory down to a zero size
    fn reset(&mut self) -> Result<(), MemoryError> {
        self.size.0 = 0;
        // update memory definition
        unsafe {
            let mut md_ptr = self.vm_memory_definition.as_ptr();
            let md = md_ptr.as_mut();
            md.current_length = 0;
        }
        Ok(())
    }

    /// Copies the memory
    /// (in this case it performs a copy-on-write to save memory)
    pub fn copy(&self) -> Result<Self, MemoryError> {
        let mem_length = self.size.bytes().0;
        let mut alloc = self
            .alloc
            .copy(Some(mem_length))
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
    // File-backed mappings cannot safely back retained host pointers.
    host_ranges: Arc<Mutex<HostRanges>>,
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
        unsafe { Self::new_internal(memory, style, None, None, MmapType::Private) }
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages
    /// that is backed by a memory file. When set to private the file will be remaining in memory and
    /// never flush to disk, when set to shared the memory will be flushed to disk.
    ///
    /// This creates a `Memory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new_with_file(
        memory: &MemoryType,
        style: &MemoryStyle,
        backing_file: std::path::PathBuf,
        memory_type: MmapType,
    ) -> Result<Self, MemoryError> {
        unsafe { Self::new_internal(memory, style, None, Some(backing_file), memory_type) }
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
        unsafe {
            Self::new_internal(
                memory,
                style,
                Some(vm_memory_location),
                None,
                MmapType::Private,
            )
        }
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages
    /// that is backed by a file. When set to private the file will be remaining in memory and
    /// never flush to disk, when set to shared the memory will be flushed to disk.
    ///
    /// This creates a `Memory` with metadata owned by a VM, pointed to by
    /// `vm_memory_location`: this can be used to create a local memory.
    ///
    /// # Safety
    /// - `vm_memory_location` must point to a valid location in VM memory.
    pub unsafe fn from_definition_with_file(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: NonNull<VMMemoryDefinition>,
        backing_file: Option<std::path::PathBuf>,
        memory_type: MmapType,
    ) -> Result<Self, MemoryError> {
        unsafe {
            Self::new_internal(
                memory,
                style,
                Some(vm_memory_location),
                backing_file,
                memory_type,
            )
        }
    }

    /// Build a `Memory` with either self-owned or VM owned metadata.
    unsafe fn new_internal(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: Option<NonNull<VMMemoryDefinition>>,
        backing_file: Option<std::path::PathBuf>,
        memory_type: MmapType,
    ) -> Result<Self, MemoryError> {
        unsafe {
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

            let offset_guard_bytes = usize::try_from(style.offset_guard_size()).map_err(|e| {
                MemoryError::Generic(format!("cannot install memory guard page: {e}"))
            })?;

            let minimum_pages = match style {
                MemoryStyle::Dynamic { .. } => memory.minimum,
                MemoryStyle::Static => {
                    let bound = MemoryStyle::static_bound();
                    assert_ge!(bound, memory.minimum);
                    bound
                }
            };
            let minimum_bytes = minimum_pages.bytes().0;
            let request_bytes = minimum_bytes.checked_add(offset_guard_bytes).unwrap();
            let mapped_pages = memory.minimum;
            let mapped_bytes = mapped_pages.bytes();

            let host_private = backing_file.is_none() && memory_type == MmapType::Private;
            let mut alloc = Mmap::accessible_reserved(
                mapped_bytes.0,
                request_bytes,
                backing_file,
                memory_type,
                true,
            )
            .map_err(MemoryError::Region)?;

            let base_ptr = alloc.as_mut_ptr();
            let mem_length = memory
                .minimum
                .bytes()
                .0
                .max(alloc.as_slice_accessible().len());
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
                    MaybeInstanceOwned::Host(Box::new(UnsafeCell::new(VMMemoryDefinition {
                        base: base_ptr,
                        current_length: mem_length,
                    })))
                },
                alloc,
                size: Bytes::from(mem_length).try_into().unwrap(),
            };

            Ok(Self {
                host_ranges: Arc::new(Mutex::new(HostRanges::new(
                    host_private,
                    (mmap.alloc.len() - offset_guard_bytes) as u64,
                ))),
                mmap,
                config: VMMemoryConfig {
                    maximum: memory.maximum,
                    offset_guard_size: offset_guard_bytes,
                    memory: *memory,
                    style: *style,
                },
            })
        }
    }

    /// Converts this owned memory into shared memory
    pub fn to_shared(self) -> VMSharedMemory {
        VMSharedMemory {
            host_ranges: self.host_ranges,
            mmap: Arc::new(RwLock::new(self.mmap)),
            config: self.config,
            conditions: ThreadConditions::new(),
        }
    }

    /// Copies this memory to a new memory
    pub fn copy(&self) -> Result<Self, MemoryError> {
        let ranges = self.host_ranges.lock().unwrap();
        ranges.can_grow()?;
        Ok(Self {
            host_ranges: Arc::new(Mutex::new(HostRanges::new(
                true,
                (self.mmap.alloc.len() - self.config.offset_guard_size) as u64,
            ))),
            mmap: self.mmap.copy()?,
            config: self.config.clone(),
        })
    }
}

// TODO: why doesn't this support wait/notify? wait should block indefinitely if you ask me
impl LinearMemory for VMOwnedMemory {
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        let minimum = self.mmap.size();
        self.config.ty(minimum)
    }

    /// Returns the size of the memory in pages
    fn size(&self) -> Pages {
        self.mmap.size()
    }

    fn begin_host_mapping_change(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<HostMemoryMappingChange, MemoryError> {
        HostRanges::begin_change(&self.host_ranges, range, || self.mmap.size.bytes().0 as u64)
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
        let ranges = self.host_ranges.lock().unwrap();
        if delta.0 != 0 {
            ranges.can_grow()?;
        }
        self.mmap.grow(delta, self.config.clone())
    }

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    fn grow_at_least(&mut self, min_size: u64) -> Result<(), MemoryError> {
        let ranges = self.host_ranges.lock().unwrap();
        if min_size > self.mmap.size.bytes().0 as u64 {
            ranges.can_grow()?;
        }
        self.mmap.grow_at_least(min_size, self.config.clone())
    }

    /// Resets the memory down to a zero size
    fn reset(&mut self) -> Result<(), MemoryError> {
        let ranges = self.host_ranges.lock().unwrap();
        ranges.can_reset()?;
        self.mmap.reset()?;
        Ok(())
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        self.mmap.vm_memory_definition.as_ptr()
    }

    /// Owned memory can not be cloned (this will always return None)
    fn try_clone(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError> {
        Err(MemoryError::MemoryNotShared)
    }

    /// Copies this memory to a new memory
    fn copy(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError> {
        let forked = Self::copy(self)?;
        Ok(Box::new(forked))
    }

    /// Return a concrete shared memory handle for detached API sharing.
    fn as_shared(&self) -> Result<VMSharedMemory, MemoryError> {
        Err(MemoryError::MemoryNotShared)
    }
}

/// Allocation-local knowledge of ranges that may safely back retained host pointers.
/// Tokens, rather than this mutex, cover physical mapping operations and callouts.
#[derive(Debug, Default)]
struct HostRanges {
    next_id: u64,
    private: Vec<std::ops::Range<u64>>,
    pins: Vec<(u64, std::ops::Range<u64>)>,
    changes: Vec<(u64, std::ops::Range<u64>)>,
}

impl HostRanges {
    fn new(private: bool, capacity: u64) -> Self {
        Self {
            private: if private {
                std::iter::once(0..capacity).collect()
            } else {
                Vec::new()
            },
            ..Self::default()
        }
    }

    fn begin_change(
        state: &Arc<Mutex<Self>>,
        range: std::ops::Range<u64>,
        size: impl FnOnce() -> u64,
    ) -> Result<HostMemoryMappingChange, MemoryError> {
        let mut ranges = state.lock().unwrap();
        if range.start >= range.end || range.end > size() || ranges.busy(&range) {
            return Err(host_range_unavailable());
        }
        let was_private = ranges
            .private
            .iter()
            .any(|known| known.start <= range.start && range.end <= known.end);
        let id = ranges.next_id()?;
        ranges.changes.push((id, range.clone()));
        ranges.remove_private(&range);
        Ok(HostMemoryMappingChange {
            ranges: state.clone(),
            id: Some(id),
            range,
            was_private,
        })
    }

    fn can_reset(&self) -> Result<(), MemoryError> {
        if self.pins.is_empty() && self.changes.is_empty() {
            Ok(())
        } else {
            Err(host_range_unavailable())
        }
    }

    fn can_grow(&self) -> Result<(), MemoryError> {
        if self.changes.is_empty() {
            Ok(())
        } else {
            Err(host_range_unavailable())
        }
    }

    fn next_id(&mut self) -> Result<u64, MemoryError> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(host_range_unavailable)?;
        Ok(self.next_id)
    }

    fn overlaps(a: &std::ops::Range<u64>, b: &std::ops::Range<u64>) -> bool {
        a.start < b.end && b.start < a.end
    }

    fn busy(&self, range: &std::ops::Range<u64>) -> bool {
        self.pins
            .iter()
            .chain(&self.changes)
            .any(|(_, other)| Self::overlaps(range, other))
    }

    fn remove_private(&mut self, range: &std::ops::Range<u64>) {
        let mut remaining = Vec::with_capacity(self.private.len() + 1);
        for old in self.private.drain(..) {
            if !Self::overlaps(&old, range) {
                remaining.push(old);
            } else {
                if old.start < range.start {
                    remaining.push(old.start..range.start);
                }
                if range.end < old.end {
                    remaining.push(range.end..old.end);
                }
            }
        }
        self.private = remaining;
    }

    fn insert_private(&mut self, mut range: std::ops::Range<u64>) {
        self.private.retain(|other| {
            if other.start <= range.end && range.start <= other.end {
                range.start = range.start.min(other.start);
                range.end = range.end.max(other.end);
                false
            } else {
                true
            }
        });
        self.private.push(range);
        self.private.sort_unstable_by_key(|range| range.start);
    }
}

fn host_range_unavailable() -> MemoryError {
    MemoryError::UnsupportedOperation {
        message: "memory range is not available for a retained host mapping".into(),
    }
}

/// Keeps an exact private, writable range of a static allocation mapped until drop.
///
/// Every physical replacement or discard must first obtain a
/// [`HostMemoryMappingChange`]. Clones of the same memory share this exclusion.
/// The token owns the allocation; no Store or process lookup is needed to use it.
pub struct HostMemoryPin {
    memory: VMSharedMemory,
    id: u64,
    range: std::ops::Range<u64>,
    // Optional wrapper resources, such as the allocation's accounting owner.
    owner: Option<Arc<dyn Send + Sync>>,
}

impl std::fmt::Debug for HostMemoryPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Inline VM metadata may already have died with its Store. The pin owns
        // the allocation, so formatting must not traverse its memory definition.
        f.debug_struct("HostMemoryPin")
            .field("id", &self.id)
            .field("range", &self.range)
            .finish()
    }
}

impl HostMemoryPin {
    /// Retains additional allocation ownership before publishing this pin.
    /// An existing owner is never replaced; rejection returns the supplied owner.
    /// Owners must not retain this pin or a Store that owns it.
    #[doc(hidden)]
    pub fn retain_owner(
        &mut self,
        owner: Arc<dyn Send + Sync>,
    ) -> Result<(), Arc<dyn Send + Sync>> {
        if self.owner.is_some() {
            return Err(owner);
        }
        self.owner = Some(owner);
        Ok(())
    }

    /// The pinned writable host address. Guest accesses may race as usual.
    pub fn as_ptr(&self) -> *mut u8 {
        let (base, _) = self.memory.host_data().expect("pinned static memory");
        // SAFETY: admission checked the range against this exact static allocation.
        unsafe { base.add(self.range.start as usize) }
    }

    /// The pinned byte range within the allocation.
    pub fn range(&self) -> std::ops::Range<u64> {
        self.range.clone()
    }
}

impl Drop for HostMemoryPin {
    fn drop(&mut self) {
        let mut ranges = self.memory.host_ranges.lock().unwrap();
        let index = ranges
            .pins
            .iter()
            .position(|(id, _)| *id == self.id)
            .expect("exact host pin remains registered");
        ranges.pins.swap_remove(index);
    }
}

/// Exclusive custody of an in-progress physical mapping change.
///
/// Admission immediately removes private-mapping knowledge. Dropping an unfinished
/// change releases exclusion but leaves the range unknown, including after errors
/// or panic in an external mapping callback. No allocation mutex crosses callbacks.
#[derive(Debug)]
pub struct HostMemoryMappingChange {
    was_private: bool,
    ranges: Arc<Mutex<HostRanges>>,
    id: Option<u64>,
    range: std::ops::Range<u64>,
}

impl HostMemoryMappingChange {
    /// Whether the entire range had known anonymous private backing on admission.
    pub fn was_private(&self) -> bool {
        self.was_private
    }

    /// Records a completed anonymous private, writable mapping.
    ///
    /// # Safety
    /// Every byte of the exact range must now have anonymous private writable
    /// backing, with no external file or shared-object owner able to revoke it.
    /// The caller must finish all physical changes and their metadata first.
    pub unsafe fn commit_private(mut self) {
        let mut ranges = self.ranges.lock().unwrap();
        let id = self.id.take().expect("mapping change is still active");
        let index = ranges
            .changes
            .iter()
            .position(|(other, _)| *other == id)
            .expect("exact mapping change remains registered");
        ranges.insert_private(self.range.clone());
        ranges.changes.swap_remove(index);
    }
}

impl Drop for HostMemoryMappingChange {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let mut ranges = self.ranges.lock().unwrap();
            let index = ranges
                .changes
                .iter()
                .position(|(other, _)| *other == id)
                .expect("exact mapping change remains registered");
            ranges.changes.swap_remove(index);
        }
    }
}

/// A shared linear memory instance.
#[derive(Debug, Clone)]
pub struct VMSharedMemory {
    // Shared by every handle of this physical allocation, including detached views.
    host_ranges: Arc<Mutex<HostRanges>>,
    // The underlying allocation.
    mmap: Arc<RwLock<WasmMmap>>,
    // Configuration of this memory
    config: VMMemoryConfig,
    // waiters list for this memory
    conditions: ThreadConditions,
}

impl VMSharedMemory {
    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages.
    ///
    /// This creates a `Memory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new(memory: &MemoryType, style: &MemoryStyle) -> Result<Self, MemoryError> {
        Ok(VMOwnedMemory::new(memory, style)?.to_shared())
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages
    /// that is backed by a file. When set to private the file will be remaining in memory and
    /// never flush to disk, when set to shared the memory will be flushed to disk.
    ///
    /// This creates a `Memory` with owned metadata: this can be used to create a memory
    /// that will be imported into Wasm modules.
    pub fn new_with_file(
        memory: &MemoryType,
        style: &MemoryStyle,
        backing_file: std::path::PathBuf,
        memory_type: MmapType,
    ) -> Result<Self, MemoryError> {
        Ok(VMOwnedMemory::new_with_file(memory, style, backing_file, memory_type)?.to_shared())
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
        unsafe {
            Ok(VMOwnedMemory::from_definition(memory, style, vm_memory_location)?.to_shared())
        }
    }

    /// Create a new linear memory instance with specified minimum and maximum number of wasm pages
    /// that is backed by a file. When set to private the file will be remaining in memory and
    /// never flush to disk, when set to shared the memory will be flushed to disk.
    ///
    /// This creates a `Memory` with metadata owned by a VM, pointed to by
    /// `vm_memory_location`: this can be used to create a local memory.
    ///
    /// # Safety
    /// - `vm_memory_location` must point to a valid location in VM memory.
    pub unsafe fn from_definition_with_file(
        memory: &MemoryType,
        style: &MemoryStyle,
        vm_memory_location: NonNull<VMMemoryDefinition>,
        backing_file: Option<std::path::PathBuf>,
        memory_type: MmapType,
    ) -> Result<Self, MemoryError> {
        unsafe {
            Ok(VMOwnedMemory::from_definition_with_file(
                memory,
                style,
                vm_memory_location,
                backing_file,
                memory_type,
            )?
            .to_shared())
        }
    }

    /// Returns host bytes only when the reservation covers every permitted growth.
    ///
    /// The base never moves while this handle exists. Dynamic memories and static
    /// memories whose maximum exceeds the static reservation return `None`.
    #[doc(hidden)]
    pub fn host_data(&self) -> Option<(*mut u8, u64)> {
        if self.config.style != MemoryStyle::Static
            || self.config.maximum.unwrap_or(Pages::max_value()) > MemoryStyle::static_bound()
        {
            return None;
        }
        // WasmMmap::grow can relocate beyond the reservation. Read allocation base
        // and size under its lock, without racing updates to VMMemoryDefinition.
        let mmap = self.mmap.read().unwrap();
        Some((mmap.alloc.as_ptr() as *mut u8, mmap.size.bytes().0 as u64))
    }

    /// Pins a known anonymous private writable range against physical replacement.
    /// Unsupported for movable memory, unknown/file-backed ranges, or overlap with
    /// another pin or mapping operation. This does not exclude ordinary guest writes.
    #[doc(hidden)]
    pub fn pin_host_range(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<HostMemoryPin, MemoryError> {
        let mut ranges = self.host_ranges.lock().unwrap();
        let (_, size) = self.host_data().ok_or_else(host_range_unavailable)?;
        if range.start >= range.end
            || range.end > size
            || ranges.busy(&range)
            || !ranges
                .private
                .iter()
                .any(|known| known.start <= range.start && range.end <= known.end)
        {
            return Err(host_range_unavailable());
        }
        let id = ranges.next_id()?;
        ranges.pins.push((id, range.clone()));
        Ok(HostMemoryPin {
            memory: self.clone(),
            id,
            range,
            owner: None,
        })
    }

    /// Atomically promotes this allocation's in-progress replacement into a pin.
    /// No other pin or mapping change can enter between commit and publication.
    ///
    /// # Safety
    /// All bytes in `mapping`'s range must have just been replaced with anonymous
    /// private writable backing; physical operations and metadata must be complete.
    #[doc(hidden)]
    pub unsafe fn commit_private_host_pin(
        &self,
        mut mapping: HostMemoryMappingChange,
    ) -> Result<HostMemoryPin, MemoryError> {
        if !Arc::ptr_eq(&self.host_ranges, &mapping.ranges) {
            return Err(host_range_unavailable());
        }
        let mut ranges = self.host_ranges.lock().unwrap();
        let (_, size) = self.host_data().ok_or_else(host_range_unavailable)?;
        if mapping.range.end > size {
            return Err(host_range_unavailable());
        }
        let id = mapping.id.take().expect("mapping change is still active");
        let index = ranges
            .changes
            .iter()
            .position(|(other, range)| *other == id && *range == mapping.range)
            .expect("exact mapping change remains registered");
        ranges.insert_private(mapping.range.clone());
        ranges.pins.push((id, mapping.range.clone()));
        ranges.changes.swap_remove(index);
        Ok(HostMemoryPin {
            memory: self.clone(),
            id,
            range: mapping.range.clone(),
            owner: None,
        })
    }

    /// Copies this memory to a new memory
    pub fn copy(&self) -> Result<Self, MemoryError> {
        let ranges = self.host_ranges.lock().unwrap();
        ranges.can_grow()?;
        let guard = self.mmap.read().unwrap();
        Ok(Self {
            host_ranges: Arc::new(Mutex::new(HostRanges::new(
                true,
                (guard.alloc.len() - self.config.offset_guard_size) as u64,
            ))),
            mmap: Arc::new(RwLock::new(guard.copy()?)),
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

    /// Returns the size of the memory in pages
    fn size(&self) -> Pages {
        let guard = self.mmap.read().unwrap();
        guard.size()
    }

    fn begin_host_mapping_change(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<HostMemoryMappingChange, MemoryError> {
        HostRanges::begin_change(&self.host_ranges, range, || {
            self.mmap.read().unwrap().size.bytes().0 as u64
        })
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
        let ranges = self.host_ranges.lock().unwrap();
        if delta.0 != 0 {
            ranges.can_grow()?;
        }
        let mut guard = self.mmap.write().unwrap();
        guard.grow(delta, self.config.clone())
    }

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    fn grow_at_least(&mut self, min_size: u64) -> Result<(), MemoryError> {
        let ranges = self.host_ranges.lock().unwrap();
        let mut guard = self.mmap.write().unwrap();
        if min_size > guard.size.bytes().0 as u64 {
            ranges.can_grow()?;
        }
        guard.grow_at_least(min_size, self.config.clone())
    }

    /// Resets the memory down to a zero size
    fn reset(&mut self) -> Result<(), MemoryError> {
        let ranges = self.host_ranges.lock().unwrap();
        ranges.can_reset()?;
        let mut guard = self.mmap.write().unwrap();
        guard.reset()?;
        Ok(())
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        let guard = self.mmap.read().unwrap();
        guard.vm_memory_definition.as_ptr()
    }

    /// Shared memory can always be cloned
    fn try_clone(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError> {
        Ok(Box::new(self.clone()))
    }

    /// Copies this memory to a new memory
    fn copy(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError> {
        let forked = Self::copy(self)?;
        Ok(Box::new(forked))
    }

    /// Return a concrete shared memory handle for detached API sharing.
    fn as_shared(&self) -> Result<VMSharedMemory, MemoryError> {
        Ok(self.clone())
    }

    // Add current thread to waiter list
    unsafe fn do_wait(
        &mut self,
        dst: u32,
        expected: ExpectedValue,
        timeout: Option<Duration>,
    ) -> Result<u32, WaiterError> {
        let dst = NotifyLocation {
            address: dst,
            memory_base: self.mmap.read().unwrap().alloc.as_ptr() as *mut _,
        };
        unsafe { self.conditions.do_wait(dst, expected, timeout) }
    }

    /// Notify waiters from the wait list. Return the number of waiters notified
    fn do_notify(&mut self, dst: u32, count: u32) -> u32 {
        self.conditions.do_notify(dst, count)
    }

    fn thread_conditions(&self) -> Option<&ThreadConditions> {
        Some(&self.conditions)
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
pub struct VMMemory(pub Box<dyn LinearMemory + Send + Sync + 'static>);

impl From<Box<dyn LinearMemory + Send + Sync + 'static>> for VMMemory {
    fn from(mem: Box<dyn LinearMemory + Send + Sync + 'static>) -> Self {
        Self(mem)
    }
}

impl LinearMemory for VMMemory {
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType {
        self.0.ty()
    }

    /// Returns the size of the memory in pages
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

    fn begin_host_mapping_change(
        &self,
        range: std::ops::Range<u64>,
    ) -> Result<HostMemoryMappingChange, MemoryError> {
        self.0.begin_host_mapping_change(range)
    }

    /// Returns the memory style for this memory.
    fn style(&self) -> MemoryStyle {
        self.0.style()
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition> {
        self.0.vmmemory()
    }

    /// Attempts to clone this memory (if its cloneable)
    fn try_clone(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError> {
        self.0.try_clone()
    }

    /// Initialize memory with data
    unsafe fn initialize_with_data(&self, start: usize, data: &[u8]) -> Result<(), Trap> {
        unsafe { self.0.initialize_with_data(start, data) }
    }

    /// Copies this memory to a new memory
    fn copy(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError> {
        self.0.copy()
    }

    fn as_shared(&self) -> Result<VMSharedMemory, MemoryError> {
        self.0.as_shared()
    }

    // Add current thread to waiter list
    unsafe fn do_wait(
        &mut self,
        dst: u32,
        expected: ExpectedValue,
        timeout: Option<Duration>,
    ) -> Result<u32, WaiterError> {
        unsafe { self.0.do_wait(dst, expected, timeout) }
    }

    /// Notify waiters from the wait list. Return the number of waiters notified
    fn do_notify(&mut self, dst: u32, count: u32) -> u32 {
        self.0.do_notify(dst, count)
    }

    fn thread_conditions(&self) -> Option<&ThreadConditions> {
        self.0.thread_conditions()
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
        unsafe {
            Ok(if memory.shared {
                Self(Box::new(VMSharedMemory::from_definition(
                    memory,
                    style,
                    vm_memory_location,
                )?))
            } else {
                Self(Box::new(VMOwnedMemory::from_definition(
                    memory,
                    style,
                    vm_memory_location,
                )?))
            })
        }
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
    pub fn copy(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError> {
        LinearMemory::copy(self)
    }

    /// Attempts to clone this memory handle.
    pub fn try_clone(&self) -> Result<Self, MemoryError> {
        LinearMemory::try_clone(self).map(Self)
    }
}

#[doc(hidden)]
/// Default implementation to initialize memory with data
pub unsafe fn initialize_memory_with_data(
    memory: &VMMemoryDefinition,
    start: usize,
    data: &[u8],
) -> Result<(), Trap> {
    unsafe {
        let mem_slice = slice::from_raw_parts_mut(memory.base, memory.current_length);
        let end = start + data.len();
        let to_init = &mut mem_slice[start..end];
        to_init.copy_from_slice(data);

        Ok(())
    }
}

/// Represents memory that is used by the WebAssembly module
pub trait LinearMemory
where
    Self: std::fmt::Debug + Send,
{
    /// Returns the type for this memory.
    fn ty(&self) -> MemoryType;

    /// Returns the size of the memory in pages
    fn size(&self) -> Pages;

    /// Admits a finite physical mapping change before any OS operation or callback.
    /// Keep the real memory alive separately until the operation and metadata commit
    /// finish. Unsupported implementations must not expose retained host pins.
    #[doc(hidden)]
    fn begin_host_mapping_change(
        &self,
        _range: std::ops::Range<u64>,
    ) -> Result<HostMemoryMappingChange, MemoryError> {
        Err(host_range_unavailable())
    }

    /// Returns the memory style for this memory.
    fn style(&self) -> MemoryStyle;

    /// Grow memory by the specified amount of wasm pages.
    ///
    /// Returns `None` if memory can't be grown by the specified amount
    /// of wasm pages.
    fn grow(&mut self, delta: Pages) -> Result<Pages, MemoryError>;

    /// Grows the memory to at least a minimum size. If the memory is already big enough
    /// for the min size then this function does nothing
    fn grow_at_least(&mut self, _min_size: u64) -> Result<(), MemoryError> {
        Err(MemoryError::UnsupportedOperation {
            message: "grow_at_least() is not supported".to_string(),
        })
    }

    /// Resets the memory back to zero length
    fn reset(&mut self) -> Result<(), MemoryError> {
        Err(MemoryError::UnsupportedOperation {
            message: "reset() is not supported".to_string(),
        })
    }

    /// Return a `VMMemoryDefinition` for exposing the memory to compiled wasm code.
    fn vmmemory(&self) -> NonNull<VMMemoryDefinition>;

    /// Attempts to clone this memory (if its cloneable)
    fn try_clone(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError>;

    #[doc(hidden)]
    /// # Safety
    /// This function is unsafe because WebAssembly specification requires that data is always set at initialization time.
    /// It should be the implementors responsibility to make sure this respects the spec
    unsafe fn initialize_with_data(&self, start: usize, data: &[u8]) -> Result<(), Trap> {
        unsafe {
            let memory = self.vmmemory().as_ref();

            initialize_memory_with_data(memory, start, data)
        }
    }

    /// Copies this memory to a new memory
    fn copy(&self) -> Result<Box<dyn LinearMemory + Send + Sync + 'static>, MemoryError>;

    /// Returns a concrete shared memory handle if this memory is shared.
    fn as_shared(&self) -> Result<VMSharedMemory, MemoryError> {
        Err(MemoryError::MemoryNotShared)
    }

    /// Add current thread to the waiter hash, and wait until notified or timeout.
    /// Return 0 if the waiter has been notified, 1 if there was a value mismatch,
    /// or 2 if the timeout occurred.
    ///
    /// # Safety
    /// the destination address must be a valid offset within this memory. It must also
    /// be properly aligned for the expected value type; either 4-byte aligned for
    /// `ExpectedValue::U32` or 8-byte aligned for `ExpectedValue::u64`.
    unsafe fn do_wait(
        &mut self,
        _dst: u32,
        _expected: ExpectedValue,
        _timeout: Option<Duration>,
    ) -> Result<u32, WaiterError> {
        Err(WaiterError::Unimplemented)
    }

    /// Notify waiters from the wait list. Return the number of waiters notified
    fn do_notify(&mut self, _dst: u32, _count: u32) -> u32 {
        0
    }

    /// Access the internal atomics handler.
    ///
    /// Will be [`None`] if the memory does not support atomics.
    fn thread_conditions(&self) -> Option<&ThreadConditions> {
        None
    }
}
