use crate::js::error::WasmError;
use crate::js::store::{AsStoreMut, AsStoreRef, InternalStoreHandle};
use crate::js::wasm_bindgen_polyfill::Global;
use js_sys::Function;
use js_sys::WebAssembly::{Memory, Table};
use std::fmt;
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{ExternType, FunctionType, GlobalType, MemoryType, TableType};

/// Represents linear memory that is managed by the javascript runtime
#[derive(Clone, Debug, PartialEq)]
pub struct VMMemory {
    pub(crate) memory: Memory,
    pub(crate) ty: MemoryType,
}

unsafe impl Send for VMMemory {}
unsafe impl Sync for VMMemory {}

impl VMMemory {
    pub(crate) fn new(memory: Memory, ty: MemoryType) -> Self {
        Self { memory, ty }
    }

    /// Attempts to clone this memory (if its clonable)
    pub(crate) fn try_clone(&self) -> Option<VMMemory> {
        Some(self.clone())
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
