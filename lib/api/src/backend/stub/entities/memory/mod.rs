use crate::backend::stub::panic_stub;
use crate::entities::store::{AsStoreMut, AsStoreRef};
use crate::shared::SharedMemory;
use crate::vm::{VMExtern, VMExternMemory, VMMemory};
use wasmer_types::{MemoryError, MemoryType, Pages};

pub mod view;
use view::MemoryView;

/// Minimal memory placeholder for the stub backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memory;

impl Memory {
    pub fn new(_store: &mut impl AsStoreMut, _ty: MemoryType) -> Result<Self, MemoryError> {
        Err(MemoryError::UnsupportedOperation {
            message: "stub backend cannot create memories".to_string(),
        })
    }

    pub fn new_from_existing(_store: &mut impl AsStoreMut, _memory: VMMemory) -> Self {
        panic_stub("cannot import existing memories")
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> MemoryType {
        panic_stub("cannot inspect memory types")
    }

    pub fn size(&self, _store: &impl AsStoreRef) -> Pages {
        panic_stub("cannot inspect memory size")
    }

    pub fn view<'a>(&self, _store: &'a impl AsStoreRef) -> MemoryView<'a> {
        panic_stub("cannot produce memory views")
    }

    pub fn grow<IntoPages>(
        &self,
        _store: &mut impl AsStoreMut,
        _delta: IntoPages,
    ) -> Result<Pages, MemoryError>
    where
        IntoPages: Into<Pages>,
    {
        Err(MemoryError::UnsupportedOperation {
            message: "stub backend cannot grow memories".to_string(),
        })
    }

    pub fn grow_at_least(
        &self,
        _store: &mut impl AsStoreMut,
        _min_size: u64,
    ) -> Result<(), MemoryError> {
        Err(MemoryError::UnsupportedOperation {
            message: "stub backend cannot grow memories".to_string(),
        })
    }

    pub fn reset(&self, _store: &mut impl AsStoreMut) -> Result<(), MemoryError> {
        Err(MemoryError::UnsupportedOperation {
            message: "stub backend cannot reset memories".to_string(),
        })
    }

    pub fn from_vm_extern(_store: &impl AsStoreRef, _vm_extern: VMExternMemory) -> Self {
        panic_stub("cannot import VM memories")
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic_stub("cannot verify memory origins")
    }

    pub fn try_clone(&self, _store: &impl AsStoreRef) -> Result<VMMemory, MemoryError> {
        Err(MemoryError::UnsupportedOperation {
            message: "stub backend cannot clone memories".to_string(),
        })
    }

    pub fn as_shared(&self, _store: &impl AsStoreRef) -> Option<SharedMemory> {
        panic_stub("cannot expose shared memories")
    }

    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Stub(crate::backend::stub::vm::VMExtern::stub())
    }
}

/// Minimal buffer view used by the stub backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryBuffer<'a> {
    _marker: std::marker::PhantomData<&'a mut [u8]>,
}

impl<'a> MemoryBuffer<'a> {
    pub fn read(&self, _offset: u64, _buf: &mut [u8]) -> Result<(), crate::MemoryAccessError> {
        panic_stub("does not support memory access")
    }

    pub fn read_uninit(
        &self,
        _offset: u64,
        _buf: &mut [std::mem::MaybeUninit<u8>],
    ) -> Result<&mut [u8], crate::MemoryAccessError> {
        panic_stub("does not support memory access")
    }

    pub fn write(&self, _offset: u64, _data: &[u8]) -> Result<(), crate::MemoryAccessError> {
        panic_stub("does not support memory access")
    }
}
