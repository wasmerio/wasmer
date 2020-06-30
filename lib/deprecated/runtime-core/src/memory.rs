use crate::{
    error::{ExportError, MemoryError},
    get_global_store, new,
    types::ValueType,
    units::Pages,
};

pub mod ptr {
    pub use crate::new::wasmer::{Array, Item, WasmPtr};
}

pub use new::wasm_common::MemoryType as MemoryDescriptor;
pub use new::wasmer::{Atomically, MemoryView};
pub use new::wasmer_runtime::MemoryStyle as MemoryType;

#[derive(Clone)]
pub struct Memory {
    new_memory: new::wasmer::Memory,
}

impl Memory {
    pub fn new(descriptor: MemoryDescriptor) -> Result<Self, MemoryError> {
        Ok(Memory {
            new_memory: new::wasmer::Memory::new(get_global_store(), descriptor)?,
        })
    }

    pub fn descriptor(&self) -> MemoryDescriptor {
        self.new_memory.ty().clone()
    }

    pub fn grow(&self, delta: Pages) -> Result<Pages, MemoryError> {
        self.new_memory.grow(delta)
    }

    pub fn size(&self) -> Pages {
        self.new_memory.size()
    }

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
    fn to_export(&self) -> new::wasmer_runtime::Export {
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
}
