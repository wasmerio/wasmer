/// This module is mainly used to create the `VM` types that will hold both
/// the JS values of the `Memory`, `Table`, `Global` and `Function` and also
/// it's types.
/// This module should not be needed any longer (with the exception of the memory)
/// once the type reflection is added to the WebAssembly JS API.
/// https://github.com/WebAssembly/js-types/
use crate::js::error::WasmError;
use crate::js::store::{AsStoreMut, AsStoreRef};
use crate::js::wasm_bindgen_polyfill::Global;
use crate::js::wasm_bindgen_polyfill::Global as JsGlobal;
use crate::MemoryView;
use js_sys::Function;
use js_sys::Function as JsFunction;
use js_sys::WebAssembly;
use js_sys::WebAssembly::{Memory, Table};
use js_sys::WebAssembly::{Memory as JsMemory, Table as JsTable};
use serde::{Deserialize, Serialize};
use std::fmt;
#[cfg(feature = "tracing")]
use tracing::trace;
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{
    ExternType, FunctionType, GlobalType, MemoryError, MemoryType, Pages, TableType, WASM_PAGE_SIZE,
};

/// Represents linear memory that is managed by the javascript runtime
#[derive(Clone, Debug, PartialEq)]
pub struct VMMemory {
    pub(crate) memory: Memory,
    pub(crate) ty: MemoryType,
}

unsafe impl Send for VMMemory {}
unsafe impl Sync for VMMemory {}

#[derive(Serialize, Deserialize)]
struct DummyBuffer {
    #[serde(rename = "byteLength")]
    byte_length: u32,
}

impl VMMemory {
    /// Creates a new memory directly from a WebAssembly javascript object
    pub fn new(memory: Memory, ty: MemoryType) -> Self {
        Self { memory, ty }
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
    pub(crate) fn try_clone(&self) -> Option<VMMemory> {
        Some(self.clone())
    }

    /// Copies this memory to a new memory
    pub fn duplicate(&self) -> Result<VMMemory, wasmer_types::MemoryError> {
        let new_memory = crate::Memory::new_internal(self.ty.clone())?;

        #[cfg(feature = "tracing")]
        trace!("memory copy started");

        let src = MemoryView::new_raw(&self.memory);
        let amount = src.data_size() as usize;
        let mut dst = MemoryView::new_raw(&new_memory);
        let dst_size = dst.data_size() as usize;

        if amount > dst_size {
            let delta = amount - dst_size;
            let pages = ((delta - 1) / WASM_PAGE_SIZE) + 1;

            let our_js_memory: &crate::js::externals::memory::JSMemory =
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

            dst = MemoryView::new_raw(&new_memory);
        }

        src.copy_to_memory(amount as u64, &dst).map_err(|err| {
            wasmer_types::MemoryError::Generic(format!("failed to copy the memory - {}", err))
        })?;

        #[cfg(feature = "tracing")]
        trace!("memory copy finished (size={})", dst.size().bytes().0);

        Ok(Self {
            memory: new_memory,
            ty: self.ty.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VMGlobal {
    pub(crate) global: Global,
    pub(crate) ty: GlobalType,
}

impl VMGlobal {
    pub(crate) fn new(global: Global, ty: GlobalType) -> Self {
        Self { global, ty }
    }
}

unsafe impl Send for VMGlobal {}
unsafe impl Sync for VMGlobal {}

#[derive(Clone, Debug, PartialEq)]
pub struct VMTable {
    pub(crate) table: Table,
    pub(crate) ty: TableType,
}

unsafe impl Send for VMTable {}
unsafe impl Sync for VMTable {}

impl VMTable {
    pub(crate) fn new(table: Table, ty: TableType) -> Self {
        Self { table, ty }
    }
    pub fn get_runtime_size(&self) -> u32 {
        self.table.length()
    }
}

#[derive(Clone)]
pub struct VMFunction {
    pub(crate) function: Function,
    pub(crate) ty: FunctionType,
}

unsafe impl Send for VMFunction {}
unsafe impl Sync for VMFunction {}

impl VMFunction {
    pub(crate) fn new(function: Function, ty: FunctionType) -> Self {
        Self { function, ty }
    }
}

impl PartialEq for VMFunction {
    fn eq(&self, other: &Self) -> bool {
        self.function == other.function
    }
}

impl fmt::Debug for VMFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VMFunction")
            .field("function", &self.function)
            .finish()
    }
}

/// The value of an export passed from one instance to another.
pub enum VMExtern {
    /// A function export value.
    Function(VMFunction),

    /// A table export value.
    Table(VMTable),

    /// A memory export value.
    Memory(VMMemory),

    /// A global export value.
    Global(VMGlobal),
}

impl VMExtern {
    /// Return the export as a `JSValue`.
    pub fn as_jsvalue<'context>(&self, _store: &'context impl AsStoreRef) -> JsValue {
        match self {
            Self::Memory(js_wasm_memory) => js_wasm_memory.memory.clone().into(),
            Self::Function(js_func) => js_func.function.clone().into(),
            Self::Table(js_wasm_table) => js_wasm_table.table.clone().into(),
            Self::Global(js_wasm_global) => js_wasm_global.global.clone().into(),
        }
    }

    /// Convert a `JsValue` into an `Export` within a given `Context`.
    pub fn from_js_value(
        val: JsValue,
        _store: &mut impl AsStoreMut,
        extern_type: ExternType,
    ) -> Result<Self, WasmError> {
        match extern_type {
            ExternType::Memory(memory_type) => {
                if val.is_instance_of::<JsMemory>() {
                    Ok(Self::Memory(VMMemory::new(
                        val.unchecked_into::<JsMemory>(),
                        memory_type,
                    )))
                } else {
                    Err(WasmError::TypeMismatch(
                        val.js_typeof()
                            .as_string()
                            .map(Into::into)
                            .unwrap_or("unknown".into()),
                        "Memory".into(),
                    ))
                }
            }
            ExternType::Global(global_type) => {
                if val.is_instance_of::<JsGlobal>() {
                    Ok(Self::Global(VMGlobal::new(
                        val.unchecked_into::<JsGlobal>(),
                        global_type,
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
            ExternType::Function(function_type) => {
                if val.is_instance_of::<JsFunction>() {
                    Ok(Self::Function(VMFunction::new(
                        val.unchecked_into::<JsFunction>(),
                        function_type,
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
            ExternType::Table(table_type) => {
                if val.is_instance_of::<JsTable>() {
                    Ok(Self::Table(VMTable::new(
                        val.unchecked_into::<JsTable>(),
                        table_type,
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
        }
    }
}

pub type VMInstance = WebAssembly::Instance;
