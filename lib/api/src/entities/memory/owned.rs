use crate::{AsStoreMut, Memory, vm::VMMemory};

/// An owned memory instance that can be attached to a store.
/// not attached to any specific store.
pub struct OwnedMemory {
    memory: VMMemory,
}

unsafe impl Send for OwnedMemory {}
unsafe impl Sync for OwnedMemory {}

impl std::fmt::Debug for OwnedMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedMemory").finish()
    }
}

impl OwnedMemory {
    /// Create a new owned memory from an existing VMMemory.
    pub(crate) fn from_vm_memory(memory: VMMemory) -> Self {
        Self { memory }
    }

    /// Attach this owned memory to the provided store.
    pub fn attach(self, store: &mut impl AsStoreMut) -> Memory {
        Memory::new_from_existing(store, self.memory)
    }
}
