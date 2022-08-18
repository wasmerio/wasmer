use crate::js::error::WasmError;
use crate::js::store::{AsStoreMut, AsStoreRef, InternalStoreHandle};
use crate::js::wasm_bindgen_polyfill::Global;
use js_sys::Function;
use js_sys::WebAssembly::{Memory, Table};
use std::fmt;
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{ExternType, FunctionType, GlobalType, MemoryType, TableType, Pages, WASM_PAGE_SIZE, StoreSnapshot};
use crate::MemoryView;
#[cfg(feature="tracing")]
use tracing::trace;

pub use wasmer_types::MemoryError;

/// Represents linear memory that is managed by the javascript runtime
#[derive(Clone, Debug, PartialEq)]
pub struct VMMemory {
    pub(crate) memory: Memory,
    pub(crate) ty: MemoryType,
}

unsafe impl Send for VMMemory {}
unsafe impl Sync for VMMemory {}

impl VMMemory {
    /// Creates a new memory directly from a WebAssembly javascript object
    pub fn new(memory: Memory, ty: MemoryType) -> Self {
        Self { memory, ty }
    }

    /// Attempts to clone this memory (if its clonable)
    pub(crate) fn try_clone(&self) -> Option<VMMemory> {
        Some(self.clone())
    }

    /// Copies this memory to a new memory
    pub fn fork(&self) -> Result<VMMemory, wasmer_types::MemoryError> {
        let new_memory = crate::Memory::new_internal(self.ty.clone())?;

        #[cfg(feature="tracing")]
        trace!("memory copy started");

        let src = MemoryView::new_raw(&self.memory);
        let amount = src.data_size() as usize;
        let mut dst = MemoryView::new_raw(&new_memory);
        let dst_size = dst.data_size() as usize;

        if amount > dst_size {
            let delta = amount - dst_size;
            let pages = ((delta - 1) / WASM_PAGE_SIZE) + 1;

            let our_js_memory: &crate::js::externals::memory::JSMemory = JsCast::unchecked_from_js_ref(&new_memory);
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

        src.copy_to_memory(amount as u64, &dst)
            .map_err(|err| {
                wasmer_types::MemoryError::Generic(format!("failed to copy the memory - {}", err))
            })?;

        #[cfg(feature="tracing")]
        trace!("memory copy finished (size={})", dst.size().bytes().0);

        Ok(
            Self {
                memory: new_memory,
                ty: self.ty.clone(),
            }
        )
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

    /// Saves the global value into the snapshot
    pub fn save_snapshot(&self, index: usize, snapshot: &mut StoreSnapshot) {
        if let Some(val) = self.global.as_f64() {
            let entry = snapshot.globals
                .entry(index as u32)
                .or_default();
            *entry = val as u128;
        }
    }

    /// Restores the global value from the snapshot
    pub fn restore_snapshot(&mut self, index: usize, snapshot: &StoreSnapshot) {
        let index = index as u32;
        if let Some(entry) = snapshot.globals.get(&index) {
            if let Some(existing) = self.global.as_f64() {
                let existing = existing as u128;
                if existing == *entry {
                    return;
                }
            }
            let value = JsValue::from_f64(*entry as _);
            self.global.set_value(&value);
        }
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

/// The value of an export passed from one instance to another.
#[derive(Debug, Clone)]
pub enum Export {
    /// A function export value.
    Function(InternalStoreHandle<VMFunction>),

    /// A table export value.
    Table(InternalStoreHandle<VMTable>),

    /// A memory export value.
    Memory(InternalStoreHandle<VMMemory>),

    /// A global export value.
    Global(InternalStoreHandle<VMGlobal>),
}

impl Export {
    /// Return the export as a `JSValue`.
    pub fn as_jsvalue<'context>(&self, store: &'context impl AsStoreRef) -> &'context JsValue {
        match self {
            Self::Memory(js_wasm_memory) => js_wasm_memory
                .get(store.as_store_ref().objects())
                .memory
                .as_ref(),
            Self::Function(js_func) => js_func
                .get(store.as_store_ref().objects())
                .function
                .as_ref(),
            Self::Table(js_wasm_table) => js_wasm_table
                .get(store.as_store_ref().objects())
                .table
                .as_ref(),
            Self::Global(js_wasm_global) => js_wasm_global
                .get(store.as_store_ref().objects())
                .global
                .as_ref(),
        }
    }

    /// Convert a `JsValue` into an `Export` within a given `Context`.
    pub fn from_js_value(
        val: JsValue,
        store: &mut impl AsStoreMut,
        extern_type: ExternType,
    ) -> Result<Self, WasmError> {
        match extern_type {
            ExternType::Memory(memory_type) => {
                if val.is_instance_of::<Memory>() {
                    Ok(Self::Memory(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMMemory::new(val.unchecked_into::<Memory>(), memory_type),
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
                if val.is_instance_of::<Global>() {
                    Ok(Self::Global(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMGlobal::new(val.unchecked_into::<Global>(), global_type),
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
            ExternType::Function(function_type) => {
                if val.is_instance_of::<Function>() {
                    Ok(Self::Function(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMFunction::new(val.unchecked_into::<Function>(), function_type),
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
            ExternType::Table(table_type) => {
                if val.is_instance_of::<Table>() {
                    Ok(Self::Table(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMTable::new(val.unchecked_into::<Table>(), table_type),
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
        }
    }
}
