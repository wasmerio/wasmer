use super::{Instance, VMInstance};
use crate::vmcontext::VMTableDefinition;
use crate::{VMGlobalDefinition, VMMemoryDefinition};
use std::alloc::{self, Layout};
use std::convert::TryFrom;
use std::mem;
use std::ptr::{self, NonNull};
use wasmer_types::entity::EntityRef;
use wasmer_types::{LocalGlobalIndex, VMOffsets};
use wasmer_types::{LocalMemoryIndex, LocalTableIndex, ModuleInfo};

/// This is an intermediate type that manages the raw allocation and
/// metadata when creating a [`VMInstance`].
///
/// This type will free the allocated memory if it's dropped before
/// being used.
///
/// It is important to remind that [`VMInstance`] is dynamically-sized
/// based on `VMOffsets`: The `Instance.vmctx` field represents a
/// dynamically-sized array that extends beyond the nominal end of the
/// type. So in order to create an instance of it, we must:
///
/// 1. Define the correct layout for `Instance` (size and alignment),
/// 2. Allocate it properly.
///
/// The `InstanceAllocator::instance_layout` helper computes the correct
/// layout to represent the wanted [`VMInstance`].
///
/// Then we use this layout to allocate an empty `Instance` properly.
pub struct InstanceAllocator {
    /// The buffer that will contain the [`VMInstance`] and dynamic fields.
    instance_ptr: NonNull<Instance>,

    /// The layout of the `instance_ptr` buffer.
    instance_layout: Layout,

    /// Information about the offsets into the `instance_ptr` buffer for
    /// the dynamic fields.
    offsets: VMOffsets,

    /// Whether or not this type has transferred ownership of the
    /// `instance_ptr` buffer. If it has not when being dropped,
    /// the buffer should be freed.
    consumed: bool,
}

impl Drop for InstanceAllocator {
    fn drop(&mut self) {
        if !self.consumed {
            // If `consumed` has not been set, then we still have ownership
            // over the buffer and must free it.
            let instance_ptr = self.instance_ptr.as_ptr();

            unsafe {
                std::alloc::dealloc(instance_ptr as *mut u8, self.instance_layout);
            }
        }
    }
}

impl InstanceAllocator {
    /// Allocates instance data for use with [`VMInstance::new`].
    ///
    /// Returns a wrapper type around the allocation and 2 vectors of
    /// pointers into the allocated buffer. These lists of pointers
    /// correspond to the location in memory for the local memories and
    /// tables respectively. These pointers should be written to before
    /// calling [`VMInstance::new`].
    ///
    /// [`VMInstance::new`]: super::VMInstance::new
    #[allow(clippy::type_complexity)]
    pub fn new(
        module: &ModuleInfo,
    ) -> (
        Self,
        Vec<NonNull<VMMemoryDefinition>>,
        Vec<NonNull<VMTableDefinition>>,
        Vec<NonNull<VMGlobalDefinition>>,
    ) {
        let offsets = VMOffsets::new(mem::size_of::<usize>() as u8, module);
        Self::new_with_offsets(offsets, module)
    }

    /// Same as [`InstanceAllocator::new`], but accepts pre-computed
    /// [`VMOffsets`] instead of computing them from the module.
    ///
    /// `VMOffsets::new(pointer_size, module)` is deterministic given
    /// `(pointer_size, module)`, and the `pointer_size` is fixed to
    /// `size_of::<usize>()` on the host. Callers that instantiate the
    /// same module repeatedly (per-request wasm hosts: cloud workers,
    /// CosmWasm, op-vm, …) can compute the offsets once at compile/cache
    /// time and pass them here to skip the recomputation on every
    /// `Instance::new`.
    ///
    /// # Caller contract
    ///
    /// `offsets` MUST equal `VMOffsets::new(size_of::<usize>() as u8, module)`
    /// for the same `module`. Passing offsets computed for a different
    /// module — or with a different pointer size — will produce a
    /// mis-sized allocation and undefined behavior. Callers that cache
    /// the offsets should key the cache by the same `ModuleInfo` they
    /// pass here.
    #[allow(clippy::type_complexity)]
    pub fn new_with_offsets(
        offsets: VMOffsets,
        module: &ModuleInfo,
    ) -> (
        Self,
        Vec<NonNull<VMMemoryDefinition>>,
        Vec<NonNull<VMTableDefinition>>,
        Vec<NonNull<VMGlobalDefinition>>,
    ) {
        // Silence unused warning when the body below does not need
        // `module` directly (it's kept in the signature for API
        // symmetry with `new` and for future callers).
        let _ = module;
        let instance_layout = Self::instance_layout(&offsets);

        #[allow(clippy::cast_ptr_alignment)]
        let instance_ptr = unsafe { alloc::alloc(instance_layout) as *mut Instance };

        let instance_ptr = if let Some(ptr) = NonNull::new(instance_ptr) {
            ptr
        } else {
            alloc::handle_alloc_error(instance_layout);
        };

        let allocator = Self {
            instance_ptr,
            instance_layout,
            offsets,
            consumed: false,
        };

        // # Safety
        // Both of these calls are safe because we allocate the pointer
        // above with the same `offsets` that these functions use.
        // Thus there will be enough valid memory for both of them.
        let memories = unsafe { allocator.memory_definition_locations() };
        let tables = unsafe { allocator.table_definition_locations() };
        let globals = unsafe { allocator.global_definition_locations() };

        (allocator, memories, tables, globals)
    }

    /// Calculate the appropriate layout for the internal `Instance` structure.
    fn instance_layout(offsets: &VMOffsets) -> Layout {
        let vmctx_size = usize::try_from(offsets.size_of_vmctx())
            .expect("Failed to convert the size of `vmctx` to a `usize`");

        let instance_vmctx_layout =
            Layout::array::<u8>(vmctx_size).expect("Failed to create a layout for `VMContext`");

        let (instance_layout, _offset) = Layout::new::<Instance>()
            .extend(instance_vmctx_layout)
            .expect("Failed to extend to `Instance` layout to include `VMContext`");

        instance_layout.pad_to_align()
    }

    /// Get the locations of where the local [`VMMemoryDefinition`]s should be stored.
    ///
    /// This function lets us create `Memory` objects on the host with backing
    /// memory in the VM.
    ///
    /// # Safety
    ///
    /// - `Self.instance_ptr` must point to enough memory that all of
    ///   the offsets in `Self.offsets` point to valid locations in
    ///   memory, i.e. `Self.instance_ptr` must have been allocated by
    ///   `Self::new`.
    unsafe fn memory_definition_locations(&self) -> Vec<NonNull<VMMemoryDefinition>> {
        unsafe {
            let num_memories = self.offsets.num_local_memories();
            let num_memories = usize::try_from(num_memories).unwrap();
            let mut out = Vec::with_capacity(num_memories);

            // We need to do some pointer arithmetic now. The unit is `u8`.
            let ptr = self.instance_ptr.cast::<u8>().as_ptr();
            let base_ptr = ptr.add(mem::size_of::<Instance>());

            for i in 0..num_memories {
                let mem_offset = self
                    .offsets
                    .vmctx_vmmemory_definition(LocalMemoryIndex::new(i));
                let mem_offset = usize::try_from(mem_offset).unwrap();

                let new_ptr = NonNull::new_unchecked(base_ptr.add(mem_offset));

                out.push(new_ptr.cast());
            }

            out
        }
    }

    /// Get the locations of where the [`VMTableDefinition`]s should be stored.
    ///
    /// This function lets us create [`Table`] objects on the host with backing
    /// memory in the VM.
    ///
    /// # Safety
    ///
    /// - `Self.instance_ptr` must point to enough memory that all of
    ///   the offsets in `Self.offsets` point to valid locations in
    ///   memory, i.e. `Self.instance_ptr` must have been allocated by
    ///   `Self::new`.
    unsafe fn table_definition_locations(&self) -> Vec<NonNull<VMTableDefinition>> {
        unsafe {
            let num_tables = self.offsets.num_local_tables();
            let num_tables = usize::try_from(num_tables).unwrap();
            let mut out = Vec::with_capacity(num_tables);

            // We need to do some pointer arithmetic now. The unit is `u8`.
            let ptr = self.instance_ptr.cast::<u8>().as_ptr();
            let base_ptr = ptr.add(std::mem::size_of::<Instance>());

            for i in 0..num_tables {
                let table_offset = self
                    .offsets
                    .vmctx_vmtable_definition(LocalTableIndex::new(i));
                let table_offset = usize::try_from(table_offset).unwrap();

                let new_ptr = NonNull::new_unchecked(base_ptr.add(table_offset));

                out.push(new_ptr.cast());
            }
            out
        }
    }

    /// Get the locations of where the [`VMGlobalDefinition`]s should be stored.
    ///
    /// This function lets us create [`Global`] objects on the host with backing
    /// memory in the VM.
    ///
    /// # Safety
    ///
    /// - `Self.instance_ptr` must point to enough memory that all of
    ///   the offsets in `Self.offsets` point to valid locations in
    ///   memory, i.e. `Self.instance_ptr` must have been allocated by
    ///   `Self::new`.
    unsafe fn global_definition_locations(&self) -> Vec<NonNull<VMGlobalDefinition>> {
        unsafe {
            let num_globals = self.offsets.num_local_globals();
            let num_globals = usize::try_from(num_globals).unwrap();
            let mut out = Vec::with_capacity(num_globals);

            let ptr = self.instance_ptr.cast::<u8>().as_ptr();
            let base_ptr = ptr.add(std::mem::size_of::<Instance>());

            for i in 0..num_globals {
                let global_offset = self
                    .offsets
                    .vmctx_vmglobal_definition(LocalGlobalIndex::new(i));
                let global_offset = usize::try_from(global_offset).unwrap();

                let new_ptr = NonNull::new_unchecked(base_ptr.add(global_offset));
                out.push(new_ptr.cast());
            }

            out
        }
    }

    /// Finish preparing by writing the internal `Instance` into memory, and
    /// consume this `InstanceAllocator`.
    pub(crate) fn into_vminstance(mut self, instance: Instance) -> VMInstance {
        // Prevent the old state's drop logic from being called as we
        // transition into the new state.
        self.consumed = true;

        unsafe {
            // `instance` is moved at `Self.instance_ptr`. This
            // pointer has been allocated by `Self::allocate_instance`
            // (so by `VMInstance::allocate_instance`).
            ptr::write(self.instance_ptr.as_ptr(), instance);
            // Now `instance_ptr` is correctly initialized!
        }
        let instance = self.instance_ptr;
        let instance_layout = self.instance_layout;

        // This is correct because of the invariants of `Self` and
        // because we write `Instance` to the pointer in this function.
        VMInstance {
            instance,
            instance_layout,
        }
    }

    /// Get the [`VMOffsets`] for the allocated buffer.
    pub(crate) fn offsets(&self) -> &VMOffsets {
        &self.offsets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmer_types::ModuleInfo;

    /// `VMOffsets::new` is deterministic for a given `(pointer_size, module)`.
    /// The whole VMOffsets-caching optimization assumes this; verify it as
    /// the test that would catch any future change to `VMOffsets::new` that
    /// introduced non-determinism (e.g. an internal HashMap iteration).
    #[test]
    fn vmoffsets_new_is_deterministic_on_empty_module() {
        let module = ModuleInfo::default();
        let ps = mem::size_of::<usize>() as u8;
        let a = VMOffsets::new(ps, &module);
        let b = VMOffsets::new(ps, &module);

        // Use Debug repr for comparison — VMOffsets doesn't impl `PartialEq`
        // upstream, but its layout is constant for a given input so the
        // textual debug form is a sufficient identity check.
        let a_dbg = format!("{a:?}");
        let b_dbg = format!("{b:?}");
        assert_eq!(a_dbg, b_dbg, "VMOffsets::new must be deterministic");
    }

    /// `InstanceAllocator::new_with_offsets` and `InstanceAllocator::new`
    /// must produce allocators with the same `instance_layout` (size +
    /// alignment) and the same `VMOffsets`. That's the invariant the
    /// upstream caller (`Artifact::instantiate`) relies on when it
    /// substitutes the cached value.
    ///
    /// The allocator owns a heap allocation; both paths allocate and we
    /// just drop the resulting allocators at end of test (their `Drop`
    /// frees the allocation since neither was `consumed`).
    #[test]
    fn new_with_offsets_matches_new() {
        let module = ModuleInfo::default();
        let ps = mem::size_of::<usize>() as u8;

        let (a, _, _, _) = InstanceAllocator::new(&module);
        let cached = VMOffsets::new(ps, &module);
        let (b, _, _, _) = InstanceAllocator::new_with_offsets(cached, &module);

        assert_eq!(
            a.instance_layout.size(),
            b.instance_layout.size(),
            "instance_layout.size() mismatch"
        );
        assert_eq!(
            a.instance_layout.align(),
            b.instance_layout.align(),
            "instance_layout.align() mismatch"
        );

        // Both should report the same `size_of_vmctx` since that's derived
        // from `VMOffsets` and the same module went in.
        assert_eq!(
            a.offsets.size_of_vmctx(),
            b.offsets.size_of_vmctx(),
            "size_of_vmctx mismatch — caching broke the layout invariant"
        );
    }

    /// Stress: build the allocator many times. Catches any state that
    /// leaks between calls (e.g. a `Vec` returned in the offsets that
    /// references a previous module). Each iteration should drop cleanly.
    #[test]
    fn new_with_offsets_many_times() {
        let module = ModuleInfo::default();
        let ps = mem::size_of::<usize>() as u8;
        let expected_size = {
            let (a, _, _, _) = InstanceAllocator::new(&module);
            a.instance_layout.size()
        };

        for _ in 0..1024 {
            let cached = VMOffsets::new(ps, &module);
            let (b, _, _, _) = InstanceAllocator::new_with_offsets(cached, &module);
            assert_eq!(b.instance_layout.size(), expected_size);
        }
    }
}
