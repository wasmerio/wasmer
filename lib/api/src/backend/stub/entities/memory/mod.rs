use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::vm::VMMemory;
use wasmer_types::{MemoryType, Pages};

pub mod view;

/// Minimal memory placeholder for the stub backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    ty: MemoryType,
}

impl Memory {
    pub fn new(_store: &mut impl AsStoreMut, ty: MemoryType) -> Result<Self, wasmer_types::MemoryError> {
        Ok(Self { ty })
    }

    pub fn ty(&self) -> &MemoryType {
        &self.ty
    }

    pub fn vm_memory(&self, _store: &impl AsStoreRef) -> VMMemory {
        panic!("stub backend does not expose VM memory")
    }

    pub fn growing_disabled(&self) -> bool {
        true
    }

    pub fn minimum_pages(&self) -> Pages {
        Pages(0)
    }
}

/// Minimal buffer view used by the stub backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryBuffer<'a> {
    _marker: std::marker::PhantomData<&'a mut [u8]>,
}

impl<'a> MemoryBuffer<'a> {
    pub fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<(), crate::MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn read_uninit(
        &self,
        _offset: u64,
        _buf: &mut [std::mem::MaybeUninit<u8>],
    ) -> Result<&mut [u8], crate::MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }

    pub fn write(&self, _offset: u64, _data: &[u8]) -> Result<(), crate::MemoryAccessError> {
        panic!("stub backend does not support memory access")
    }
}
