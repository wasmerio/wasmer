use crate::{error::MemoryError, new, types::ValueType, units::Pages};

pub mod ptr {
    pub use crate::new::wasmer::{Array, Item, WasmPtr};
}

pub use new::wasm_common::MemoryType as MemoryDescriptor;
pub use new::wasmer::MemoryView;
pub use new::wasmer_runtime::MemoryStyle as MemoryType;

pub struct Memory {
    new_memory: new::wasmer::Memory,
}

impl Memory {
    pub fn new(descriptor: MemoryDescriptor) -> Result<Self, MemoryError> {
        let store = Default::default();

        Ok(Memory {
            new_memory: new::wasmer::Memory::new(&store, descriptor)?,
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
