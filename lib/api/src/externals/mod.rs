pub(crate) mod function;
mod global;
mod memory;
mod table;

pub use self::function::{
    FromToNativeWasmType, Function, HostFunction, WasmTypeList, WithEnv, WithoutEnv,
};
pub use self::global::Global;
pub use self::memory::Memory;
pub use self::table::Table;

use crate::exports::{ExportError, Exportable};
use crate::store::{Store, StoreObject};
use crate::ExternType;
use std::fmt;
use wasmer_vm::Export;

/// An `Extern` is the runtime representation of an entity that
/// can be imported or exported.
///
/// Spec: https://webassembly.github.io/spec/core/exec/runtime.html#external-values
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
    /// Return the undelying type of the inner `Extern`.
    pub fn ty(&self) -> ExternType {
        match self {
            Extern::Function(ft) => ExternType::Function(ft.ty().clone()),
            Extern::Memory(ft) => ExternType::Memory(*ft.ty()),
            Extern::Table(tt) => ExternType::Table(*tt.ty()),
            Extern::Global(gt) => ExternType::Global(*gt.ty()),
        }
    }

    /// Create an `Extern` from an `Export`.
    pub fn from_export(store: &Store, export: Export) -> Extern {
        match export {
            Export::Function(f) => Extern::Function(Function::from_export(store, f)),
            Export::Memory(m) => Extern::Memory(Memory::from_export(store, m)),
            Export::Global(g) => Extern::Global(Global::from_export(store, g)),
            Export::Table(t) => Extern::Table(Table::from_export(store, t)),
        }
    }
}

impl<'a> Exportable<'a> for Extern {
    fn to_export(&self) -> Export {
        match self {
            Extern::Function(f) => f.to_export(),
            Extern::Global(g) => g.to_export(),
            Extern::Memory(m) => m.to_export(),
            Extern::Table(t) => t.to_export(),
        }
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        // Since this is already an extern, we can just return it.
        Ok(_extern)
    }
}

impl StoreObject for Extern {
    fn comes_from_same_store(&self, store: &Store) -> bool {
        let my_store = match self {
            Extern::Function(f) => f.store(),
            Extern::Global(g) => g.store(),
            Extern::Memory(m) => m.store(),
            Extern::Table(t) => t.store(),
        };
        Store::same(my_store, store)
    }
}

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
        Extern::Function(r)
    }
}

impl From<Global> for Extern {
    fn from(r: Global) -> Self {
        Extern::Global(r)
    }
}

impl From<Memory> for Extern {
    fn from(r: Memory) -> Self {
        Extern::Memory(r)
    }
}

impl From<Table> for Extern {
    fn from(r: Table) -> Self {
        Extern::Table(r)
    }
}
