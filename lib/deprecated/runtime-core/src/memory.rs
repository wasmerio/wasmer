use crate::{
    error::{ExportError, MemoryError},
    get_global_store, new,
    types::ValueType,
    units::Pages,
};

pub mod ptr {
    pub use crate::new::wasmer::{Array, Item, WasmPtr};
}

pub use new::wasmer::{Atomically, MemoryView};
pub use new::wasmer_types::MemoryType as MemoryDescriptor;
pub use new::wasmer_vm::MemoryStyle as MemoryType;

/// A Wasm linear memory.
///
/// A `Memory` represents the memory used by a Wasm instance.
#[derive(Clone)]
pub struct Memory {
    new_memory: new::wasmer::Memory,
}

impl Memory {
    /// Create a new `Memory` from a [`MemoryDescriptor`]
    ///
    /// [`MemoryDescriptor`]: struct.MemoryDescriptor.html
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::{types::MemoryDescriptor, error::MemoryError, memory::Memory, units::Pages};
    /// fn create_memory() -> Result<(), MemoryError> {
    ///     let descriptor = MemoryDescriptor::new(Pages(10), None, false);
    ///
    ///     let memory = Memory::new(descriptor)?;
    ///     Ok(())
    /// }
    /// ```
    pub fn new(descriptor: MemoryDescriptor) -> Result<Self, MemoryError> {
        Ok(Memory {
            new_memory: new::wasmer::Memory::new(&get_global_store(), descriptor)?,
        })
    }

    /// Return the [`MemoryDescriptor`] that this memory
    /// was created with.
    ///
    /// [`MemoryDescriptor`]: struct.MemoryDescriptor.html
    pub fn descriptor(&self) -> MemoryDescriptor {
        self.new_memory.ty().clone()
    }

    /// Grow this memory by the specified number of pages.
    pub fn grow(&self, delta: Pages) -> Result<Pages, MemoryError> {
        self.new_memory.grow(delta)
    }

    /// The size, in wasm pages, of this memory.
    pub fn size(&self) -> Pages {
        self.new_memory.size()
    }

    /// Return a "view" of the currently accessible memory. By
    /// default, the view is unsynchronized, using regular memory
    /// accesses. You can force a memory view to use atomic accesses
    /// by calling the [`atomically`] method.
    ///
    /// [`atomically`]: struct.MemoryView.html#method.atomically
    ///
    /// # Notes
    ///
    /// This method is safe (as in, it won't cause the host to crash or have UB),
    /// but it doesn't obey rust's rules involving data races, especially concurrent ones.
    /// Therefore, if this memory is shared between multiple threads, a single memory
    /// location can be mutated concurrently without synchronization.
    ///
    /// # Usage
    ///
    /// ```
    /// # use wasmer_runtime_core::memory::{Memory, MemoryView};
    /// # use std::{cell::Cell, sync::atomic::Ordering};
    /// # fn view_memory(memory: Memory) {
    /// // Without synchronization.
    /// let view: MemoryView<u8> = memory.view();
    /// for byte in view[0x1000 .. 0x1010].iter().map(Cell::get) {
    ///     println!("byte: {}", byte);
    /// }
    ///
    /// // With synchronization.
    /// let atomic_view = view.atomically();
    /// for byte in atomic_view[0x1000 .. 0x1010].iter().map(|atom| atom.load(Ordering::SeqCst)) {
    ///     println!("byte: {}", byte);
    /// }
    /// # }
    /// ```
    pub fn view<T: ValueType>(&self) -> MemoryView<T> {
        self.new_memory.view()
    }
}

impl From<&new::wasmer::Memory> for Memory {
    fn from(new_memory: &new::wasmer::Memory) -> Self {
        Self {
            new_memory: new_memory.clone(),
        }
    }
}

impl<'a> new::wasmer::Exportable<'a> for Memory {
    fn to_export(&self) -> new::wasmer::Export {
        self.new_memory.to_export()
    }

    fn get_self_from_extern(r#extern: &'a new::wasmer::Extern) -> Result<&'a Self, ExportError> {
        match r#extern {
            new::wasmer::Extern::Memory(memory) => Ok(
                // It's not ideal to call `Box::leak` here, but it
                // would introduce too much changes in the
                // `new::wasmer` API to support `Cow` or similar.
                Box::leak(Box::<Memory>::new(memory.into())),
            ),
            _ => Err(ExportError::IncompatibleType),
        }
    }

    fn into_weak_instance_ref(&mut self) {
        self.new_memory.into_weak_instance_ref();
    }
}

#[cfg(test)]
mod memory_tests {
    use super::{Memory, MemoryDescriptor, Pages};

    #[test]
    fn test_initial_memory_size() {
        let memory_desc = MemoryDescriptor::new(Pages(10), Some(Pages(20)), false);
        let unshared_memory = Memory::new(memory_desc).unwrap();

        assert_eq!(unshared_memory.size(), Pages(10));
    }
}
