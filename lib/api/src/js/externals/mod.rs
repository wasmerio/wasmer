pub(crate) mod function;
mod global;
pub(crate) mod memory;
pub(crate) mod memory_view;
mod table;

pub use self::function::{FromToNativeWasmType, Function, HostFunction, WasmTypeList};
pub use self::global::Global;
pub use self::memory::{Memory, MemoryError};
pub use self::memory_view::MemoryView;
pub use self::table::Table;

use crate::js::export::{Export, VMFunction, VMGlobal, VMMemory, VMTable};
use crate::js::exports::{ExportError, Exportable};
use crate::js::store::StoreObject;
use crate::js::types::AsJs;

/*


use crate::js::store::InternalStoreHandle;
use crate::js::store::{AsStoreMut, AsStoreRef};
use crate::js::ExternType;
use std::fmt;
*/
use crate::js::error::WasmError;
use crate::js::store::{AsStoreMut, AsStoreRef, InternalStoreHandle};
use crate::js::wasm_bindgen_polyfill::Global as JsGlobal;
use js_sys::Function as JsFunction;
use js_sys::WebAssembly::{Memory as JsMemory, Table as JsTable};
use std::fmt;
use wasm_bindgen::{JsCast, JsValue};
use wasmer_types::{ExternType, FunctionType, GlobalType, MemoryType, TableType};

/// The value of an export passed from one instance to another.
pub enum VMExtern {
    /// A function export value.
    Function(InternalStoreHandle<VMFunction>),

    /// A table export value.
    Table(InternalStoreHandle<VMTable>),

    /// A memory export value.
    Memory(InternalStoreHandle<VMMemory>),

    /// A global export value.
    Global(InternalStoreHandle<VMGlobal>),
}

impl VMExtern {
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
                if val.is_instance_of::<JsMemory>() {
                    Ok(Self::Memory(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMMemory::new(val.unchecked_into::<JsMemory>(), memory_type),
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
                    Ok(Self::Global(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMGlobal::new(val.unchecked_into::<JsGlobal>(), global_type),
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
            ExternType::Function(function_type) => {
                if val.is_instance_of::<JsFunction>() {
                    Ok(Self::Function(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMFunction::new(val.unchecked_into::<JsFunction>(), function_type),
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
            ExternType::Table(table_type) => {
                if val.is_instance_of::<JsTable>() {
                    Ok(Self::Table(InternalStoreHandle::new(
                        &mut store.objects_mut(),
                        VMTable::new(val.unchecked_into::<JsTable>(), table_type),
                    )))
                } else {
                    panic!("Extern type doesn't match js value type");
                }
            }
        }
    }
}

/// An `Extern` is the runtime representation of an entity that
/// can be imported or exported.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#external-values>
#[derive(Clone)]
pub enum Extern {
    /// A external [`Function`].
    Function(Function),
    /// A external [`Global`].
    Global(Global),
    /// A external [`Table`].
    Table(Table),
    /// A external [`Memory`].
    Memory(Memory),
}

impl Extern {
    /// Return the underlying type of the inner `Extern`.
    pub fn ty(&self, store: &impl AsStoreRef) -> ExternType {
        match self {
            Self::Function(ft) => ExternType::Function(ft.ty(store).clone()),
            Self::Memory(ft) => ExternType::Memory(ft.ty(store)),
            Self::Table(tt) => ExternType::Table(tt.ty(store)),
            Self::Global(gt) => ExternType::Global(gt.ty(store)),
        }
    }

    /// Create an `Extern` from an `wasmer_engine::Export`.
    pub fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExtern) -> Self {
        match vm_extern {
            VMExtern::Function(f) => Self::Function(Function::from_vm_extern(store, f)),
            VMExtern::Memory(m) => Self::Memory(Memory::from_vm_extern(store, m)),
            VMExtern::Global(g) => Self::Global(Global::from_vm_extern(store, g)),
            VMExtern::Table(t) => Self::Table(Table::from_vm_extern(store, t)),
        }
    }

    /// To `VMExtern`.
    pub fn to_vm_extern(&self) -> VMExtern {
        match self {
            Self::Function(f) => f.to_vm_extern(),
            Self::Global(g) => g.to_vm_extern(),
            Self::Memory(m) => m.to_vm_extern(),
            Self::Table(t) => t.to_vm_extern(),
        }
    }

    /// Checks whether this `Extern` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match self {
            Self::Function(val) => val.is_from_store(store),
            Self::Memory(val) => val.is_from_store(store),
            Self::Global(val) => val.is_from_store(store),
            Self::Table(val) => val.is_from_store(store),
        }
    }

    fn to_export(&self) -> Export {
        match self {
            Self::Function(val) => Export::Function(val.handle.internal_handle()),
            Self::Memory(val) => Export::Memory(val.handle.internal_handle()),
            Self::Global(val) => Export::Global(val.handle.internal_handle()),
            Self::Table(val) => Export::Table(val.handle.internal_handle()),
        }
    }
}

impl AsJs for Extern {
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
        match self {
            Self::Function(_) => self.to_export().as_jsvalue(store),
            Self::Global(_) => self.to_export().as_jsvalue(store),
            Self::Table(_) => self.to_export().as_jsvalue(store),
            Self::Memory(_) => self.to_export().as_jsvalue(store),
        }
        .clone()
    }
}

impl<'a> Exportable<'a> for Extern {
    fn get_self_from_extern(_extern: &'a Self) -> Result<&'a Self, ExportError> {
        // Since this is already an extern, we can just return it.
        Ok(_extern)
    }
}

impl StoreObject for Extern {}

impl fmt::Debug for Extern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Function(_) => "Function(...)",
                Self::Global(_) => "Global(...)",
                Self::Memory(_) => "Memory(...)",
                Self::Table(_) => "Table(...)",
            }
        )
    }
}

impl From<Function> for Extern {
    fn from(r: Function) -> Self {
        Self::Function(r)
    }
}

impl From<Global> for Extern {
    fn from(r: Global) -> Self {
        Self::Global(r)
    }
}

impl From<Memory> for Extern {
    fn from(r: Memory) -> Self {
        Self::Memory(r)
    }
}

impl From<Table> for Extern {
    fn from(r: Table) -> Self {
        Self::Table(r)
    }
}
