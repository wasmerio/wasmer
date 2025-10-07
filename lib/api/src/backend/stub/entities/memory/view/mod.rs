use std::{marker::PhantomData, vec::Vec};

use crate::MemoryAccessError;
use wasmer_types::Pages;

use crate::backend::stub::entities::memory::MemoryBuffer;

/// Minimal memory view placeholder for the stub backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryView<'a> {
    _marker: PhantomData<&'a mut [u8]>,
}

impl<'a> MemoryView<'a> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    pub fn buffer(&self) -> MemoryBuffer<'a> {
        MemoryBuffer::default()
    }

    pub fn data_ptr(&self) -> *mut u8 {
        panic!("stub backend memory view has no data pointer")
    }

    pub fn data_size(&self) -> u64 {
        0
    }

    pub unsafe fn data_unchecked(&self) -> &[u8] {
        panic!("stub backend memory view cannot expose raw data")
    }

    pub unsafe fn data_unchecked_mut(&self) -> &mut [u8] {
        panic!("stub backend memory view cannot expose raw mutable data")
    }

    pub fn size(&self) -> Pages {
        Pages(0)
    }

    pub fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<(), MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn read_u8(&self, _offset: u64) -> Result<u8, MemoryAccessError> {
        Err(MemoryAccessError::HeapOutOfBounds)
    }

    pub fn read_uninit<'b>(
        &self,
        _offset: u64,
        _buf: &'b mut [std::mem::MaybeUninit<u8>],
    ) -> Result<&'b mut [u8], MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn write(&self, _offset: u64, _data: &[u8]) -> Result<(), MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn write_u8(&self, _offset: u64, _value: u8) -> Result<(), MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn copy_to_vec(&self) -> Result<Vec<u8>, MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn copy_range_to_vec(
        &self,
        _range: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn copy_to_memory(
        &self,
        _amount: u64,
        _new_memory: &Self,
    ) -> Result<(), MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }
}
