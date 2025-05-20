use wasmer_types::MemoryError;

use crate::error::AtomicsError;

/// Location in a WebAssembly memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemoryLocation {
    // NOTE: must be expanded to an enum that also supports 64bit memory in
    // the future
    // That's why this is private.
    pub(crate) address: u32,
}

impl MemoryLocation {
    /// Create a new memory location for a 32bit memory.
    pub fn new_32(address: u32) -> Self {
        Self { address }
    }
}

impl From<u32> for MemoryLocation {
    fn from(value: u32) -> Self {
        Self::new_32(value)
    }
}

/// See [`crate::SharedMemory`].
pub(crate) trait SharedMemoryOps {
    /// See [`crate::SharedMemory::disable_atomics`].
    fn disable_atomics(&self) -> Result<(), MemoryError> {
        Err(MemoryError::AtomicsNotSupported)
    }

    /// See [`crate::SharedMemory::wake_all_atomic_waiters`].
    fn wake_all_atomic_waiters(&self) -> Result<(), MemoryError> {
        Err(MemoryError::AtomicsNotSupported)
    }

    /// See [`crate::SharedMemory::notify`].
    fn notify(&self, _dst: MemoryLocation, _count: u32) -> Result<u32, AtomicsError> {
        Err(AtomicsError::Unimplemented)
    }

    /// See [`crate::SharedMemory::wait`].
    fn wait(
        &self,
        _dst: MemoryLocation,
        _timeout: Option<std::time::Duration>,
    ) -> Result<u32, AtomicsError> {
        Err(AtomicsError::Unimplemented)
    }
}
