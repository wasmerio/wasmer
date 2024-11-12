use std::mem::MaybeUninit;

use crate::MemoryAccessError;

/// Underlying buffer for a memory.
#[derive(Debug)]
pub(crate) struct MemoryBuffer(pub(crate) Box<dyn MemoryBufferLike>);

impl MemoryBuffer {
    #[allow(unused)]
    pub(crate) fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        self.0.read(offset, buf)
    }

    #[allow(unused)]
    pub(crate) fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        self.0.read_uninit(offset, buf)
    }

    #[allow(unused)]
    pub(crate) fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError> {
        self.0.write(offset, data)
    }
}

impl Clone for MemoryBuffer {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

/// The trait that every concrete memory buffer must implement.
pub trait MemoryBufferLike: std::fmt::Debug {
    #[allow(unused)]
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<(), MemoryAccessError>;

    #[allow(unused)]
    fn read_uninit<'b>(
        &self,
        offset: u64,
        buf: &'b mut [MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError>;

    #[allow(unused)]
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), MemoryAccessError>;

    /// Create a boxed clone of this implementer.
    fn clone_box(&self) -> Box<dyn MemoryBufferLike>;

    fn len(&self) -> usize;

    fn base(&self) -> *mut u8;
}
