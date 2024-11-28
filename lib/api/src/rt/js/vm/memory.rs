use crate::js::utils::js_handle::JsHandle;
use js_sys::WebAssembly::Memory as JsMemory;
use tracing::trace;
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{MemoryError, MemoryType, Pages, WASM_PAGE_SIZE};

/// Represents linear memory that is managed by the javascript runtime
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VMMemory {
    pub(crate) memory: JsHandle<JsMemory>,
    pub(crate) ty: MemoryType,
}

unsafe impl Send for VMMemory {}
unsafe impl Sync for VMMemory {}

#[derive(serde::Serialize, serde::Deserialize)]
struct DummyBuffer {
    #[serde(rename = "byteLength")]
    byte_length: u32,
}

impl VMMemory {
    /// Creates a new memory directly from a WebAssembly javascript object
    pub fn new(memory: JsMemory, ty: MemoryType) -> Self {
        Self {
            memory: JsHandle::new(memory),
            ty,
        }
    }

    /// Returns the size of the memory buffer in pages
    pub fn get_runtime_size(&self) -> u32 {
        let dummy: DummyBuffer = match serde_wasm_bindgen::from_value(self.memory.buffer()) {
            Ok(o) => o,
            Err(_) => return 0,
        };
        if dummy.byte_length == 0 {
            return 0;
        }
        dummy.byte_length / WASM_PAGE_SIZE as u32
    }

    /// Attempts to clone this memory (if its clonable)
    pub(crate) fn try_clone(&self) -> Result<VMMemory, MemoryError> {
        Ok(self.clone())
    }

    /// Copies this memory to a new memory
    pub fn copy(&mut self) -> Result<VMMemory, wasmer_types::MemoryError> {
        let new_memory = crate::js::memory::Memory::js_memory_from_type(&self.ty)?;

        let src = crate::js::memory::MemoryView::new_raw(&self.memory);
        let amount = src.data_size() as usize;

        trace!(%amount, "memory copy started");

        let mut dst = crate::js::memory::MemoryView::new_raw(&new_memory);
        let dst_size = dst.data_size() as usize;

        if amount > dst_size {
            let delta = amount - dst_size;
            let pages = ((delta - 1) / WASM_PAGE_SIZE) + 1;

            let our_js_memory: &crate::js::memory::JSMemory =
                JsCast::unchecked_from_js_ref(&new_memory);
            our_js_memory.grow(pages as u32).map_err(|err| {
                if err.is_instance_of::<js_sys::RangeError>() {
                    let cur_pages = dst_size;
                    MemoryError::CouldNotGrow {
                        current: Pages(cur_pages as u32),
                        attempted_delta: Pages(pages as u32),
                    }
                } else {
                    MemoryError::Generic(err.as_string().unwrap())
                }
            })?;

            dst = crate::js::memory::MemoryView::new_raw(&new_memory);
        }

        src.copy_to_memory(amount as u64, &dst).map_err(|err| {
            wasmer_types::MemoryError::Generic(format!("failed to copy the memory - {}", err))
        })?;

        trace!("memory copy finished (size={})", dst.size().bytes().0);

        Ok(Self {
            memory: JsHandle::new(new_memory),
            ty: self.ty.clone(),
        })
    }
}

impl From<VMMemory> for JsValue {
    fn from(value: VMMemory) -> Self {
        JsValue::from(value.memory)
    }
}

impl From<VMMemory> for (JsValue, MemoryType) {
    fn from(value: VMMemory) -> Self {
        (JsValue::from(value.memory), value.ty)
    }
}

/// Shared VM memory, in `js`, is the "normal" memory.
pub type VMSharedMemory = VMMemory;
