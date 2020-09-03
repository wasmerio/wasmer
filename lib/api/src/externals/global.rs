use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::{Store, StoreObject};
use crate::types::Val;
use crate::GlobalType;
use crate::Mutability;
use crate::RuntimeError;
use std::fmt;
use std::sync::Arc;
use wasmer_vm::{Export, ExportGlobal, Global as RuntimeGlobal};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: https://webassembly.github.io/spec/core/exec/runtime.html#global-instances
#[derive(Clone)]
pub struct Global {
    store: Store,
    global: Arc<RuntimeGlobal>,
}

impl Global {
    /// Create a new `Global` with the initial value [`Val`].
    pub fn new(store: &Store, val: Val) -> Global {
        Self::from_value(store, val, Mutability::Const).unwrap()
    }

    /// Create a mutable `Global` with the initial value [`Val`].
    pub fn new_mut(store: &Store, val: Val) -> Global {
        Self::from_value(store, val, Mutability::Var).unwrap()
    }

    /// Create a `Global` with the initial value [`Val`] and the provided [`Mutability`].
    fn from_value(store: &Store, val: Val, mutability: Mutability) -> Result<Global, RuntimeError> {
        if !val.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` globals are not supported"));
        }
        let global = RuntimeGlobal::new(GlobalType {
            mutability,
            ty: val.ty(),
        });
        unsafe {
            global
                .set_unchecked(val.clone())
                .map_err(|e| RuntimeError::new(format!("create global for {:?}: {}", val, e)))?;
        };

        Ok(Global {
            store: store.clone(),
            global: Arc::new(global),
        })
    }

    /// Returns the [`GlobalType`] of the `Global`.
    pub fn ty(&self) -> &GlobalType {
        self.global.ty()
    }

    /// Returns the [`Store`] where the `Global` belongs.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Retrieves the current value [`Val`] that the Global has.
    pub fn get(&self) -> Val {
        self.global.get()
    }

    /// Sets a custom value [`Val`] to the runtime Global.
    ///
    /// # Errors
    ///
    /// This function will error if:
    /// * The global is not mutable
    /// * The type of the `Val` doesn't matches the Global type.
    pub fn set(&self, val: Val) -> Result<(), RuntimeError> {
        if !val.comes_from_same_store(&self.store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        unsafe {
            self.global
                .set(val)
                .map_err(|e| RuntimeError::new(format!("{}", e)))?;
        }
        Ok(())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportGlobal) -> Global {
        Global {
            store: store.clone(),
            global: wasmer_export.from.clone(),
        }
    }

    /// Returns whether or not these two globals refer to the same data.
    pub fn same(&self, other: &Global) -> bool {
        Arc::ptr_eq(&self.global, &other.global)
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
        ExportGlobal {
            from: self.global.clone(),
        }
        .into()
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
