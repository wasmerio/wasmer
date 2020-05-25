pub mod ptr {
    pub use crate::new::wasmer::{Array, Item, WasmPtr};
}

pub use crate::new::wasm_common::MemoryType as MemotyDescriptor;
pub use crate::new::wasmer_runtime::MemoryStyle as MemoryType;
