pub(crate) mod function;
mod global;
pub(crate) mod memory;
mod table;

pub use self::function::{FromToNativeWasmType, Function, HostFunction, WasmTypeList};
pub use self::global::Global;
pub use self::memory::{Memory, MemoryError};
pub use self::table::Table;

use crate::js::export::Export;
use crate::js::exports::{ExportError, Exportable};
use crate::js::store::StoreObject;
use crate::js::store::{AsStoreMut, AsStoreRef};
use crate::js::types::AsJs;
use crate::js::ExternType;
use std::fmt;

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

    /// Create an `Extern` from an `wasmer_compiler::Export`.
    pub fn from_vm_export(store: &mut impl AsStoreMut, export: Export) -> Self {
        match export {
            Export::Function(f) => Self::Function(Function::from_vm_extern(store, f)),
            Export::Memory(m) => Self::Memory(Memory::from_vm_extern(store, m)),
            Export::Global(g) => Self::Global(Global::from_vm_extern(store, g)),
            Export::Table(t) => Self::Table(Table::from_vm_extern(store, t)),
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
