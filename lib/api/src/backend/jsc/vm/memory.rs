use rusty_jsc::JSObject;
use tracing::trace;
use wasmer_types::{MemoryError, MemoryType};

use crate::AsStoreRef;

/// Represents linear memory that is managed by the Javascript Core runtime
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VMMemory {
    pub(crate) memory: JSObject,
    pub(crate) ty: MemoryType,
}

unsafe impl Send for VMMemory {}
unsafe impl Sync for VMMemory {}

impl VMMemory {
    /// Creates a new memory directly from a WebAssembly javascript object
    pub fn new(memory: JSObject, ty: MemoryType) -> Self {
        Self { memory, ty }
    }

    /// Returns the size of the memory buffer in pages
    pub fn get_runtime_size(&self) -> u32 {
        unimplemented!();
        // let dummy: DummyBuffer = match serde_wasm_bindgen::from_value(self.memory.buffer()) {
        //     Ok(o) => o,
        //     Err(_) => return 0,
        // };
        // if dummy.byte_length == 0 {
        //     return 0;
        // }
        // dummy.byte_length / WASM_PAGE_SIZE as u32
    }

    /// Attempts to clone this memory (if its clonable)
    pub(crate) fn try_clone(&self) -> Result<VMMemory, MemoryError> {
        Ok(self.clone())
    }

    /// Copies this memory to a new memory
    pub fn copy(&self, store: &impl AsStoreRef) -> Result<VMMemory, wasmer_types::MemoryError> {
        let new_memory = crate::jsc::memory::Memory::js_memory_from_type(&store, &self.ty)?;

        trace!("memory copy started");

        let src = crate::jsc::memory::view::MemoryView::new_raw(&self.memory, store);
        let amount = src.data_size() as usize;
        let mut dst = crate::jsc::memory::view::MemoryView::new_raw(&new_memory, store);
        let dst_size = dst.data_size() as usize;

        src.copy_to_memory(amount as u64, &dst).map_err(|err| {
            wasmer_types::MemoryError::Generic(format!("failed to copy the memory - {}", err))
        })?;

        trace!("memory copy finished (size={})", dst.size().bytes().0);

        Ok(Self {
            memory: new_memory,
            ty: self.ty.clone(),
        })
    }
}

/// Shared VM memory, in `jsc`, is the "normal" memory.
pub type VMSharedMemory = VMMemory;
