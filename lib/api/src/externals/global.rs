use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::{Store, StoreObject};
use crate::types::Val;
use crate::GlobalType;
use crate::Mutability;
use crate::RuntimeError;
use std::fmt;
use std::sync::Arc;
use wasmer_runtime::{Export, ExportGlobal, Global as RuntimeGlobal};

#[derive(Clone)]
pub struct Global {
    store: Store,
    exported: ExportGlobal,
    // This should not be here;
    // it should be accessed through `exported`
    // vm_global_definition: Arc<UnsafeCell<VMGlobalDefinition>>,
}

impl Global {
    pub fn new(store: &Store, val: Val) -> Global {
        Self::from_type(store, Mutability::Const, val).unwrap()
    }

    pub fn new_mut(store: &Store, val: Val) -> Global {
        Self::from_type(store, Mutability::Var, val).unwrap()
    }

    fn from_type(store: &Store, mutability: Mutability, val: Val) -> Result<Global, RuntimeError> {
        if !val.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` globals are not supported"));
        }
        let from = Arc::new(RuntimeGlobal::new_with_value(mutability, val));
        let definition = from.vmglobal();
        let exported = ExportGlobal { definition, from };
        Ok(Global {
            store: store.clone(),
            exported,
        })
    }

    pub fn ty(&self) -> &GlobalType {
        self.exported.from.ty()
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn get(&self) -> Val {
        self.exported.from.get()
    }

    pub fn set(&self, val: Val) -> Result<(), RuntimeError> {
        if !val.comes_from_same_store(&self.store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        unsafe {
            self.exported
                .from
                .set(val)
                .map_err(|e| RuntimeError::new(e.to_string()))?
        };
        Ok(())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportGlobal) -> Global {
        Global {
            store: store.clone(),
            exported: wasmer_export,
        }
    }

    /// Returns whether or not these two globals refer to the same data.
    pub fn same(&self, other: &Global) -> bool {
        self.exported.same(&other.exported)
    }
}

impl fmt::Debug for Global {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter
            .debug_struct("Global")
            .field("ty", &self.ty())
            .field("value", &self.get())
            .finish()
    }
}

impl<'a> Exportable<'a> for Global {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
