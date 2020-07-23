use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::{Store, StoreObject};
use crate::types::{Val, ValType};
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
            match val {
                Val::I32(x) => *global.get_mut().as_i32_mut() = x,
                Val::I64(x) => *global.get_mut().as_i64_mut() = x,
                Val::F32(x) => *global.get_mut().as_f32_mut() = x,
                Val::F64(x) => *global.get_mut().as_f64_mut() = x,
                _ => return Err(RuntimeError::new(format!("create_global for {:?}", val))),
                // Val::V128(x) => *definition.as_u128_bits_mut() = x,
            }
        };

        let definition = global.vmglobal();
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
        unsafe {
            let definition = self.global.get();
            match self.ty().ty {
                ValType::I32 => Val::from(*definition.as_i32()),
                ValType::I64 => Val::from(*definition.as_i64()),
                ValType::F32 => Val::F32(*definition.as_f32()),
                ValType::F64 => Val::F64(*definition.as_f64()),
                _ => unimplemented!("Global::get for {:?}", self.ty().ty),
            }
        }
    }

    /// Sets a custom value [`Val`] to the runtime Global.
    ///
    /// # Errors
    ///
    /// This function will error if:
    /// * The global is not mutable
    /// * The type of the `Val` doesn't matches the Global type.
    pub fn set(&self, val: Val) -> Result<(), RuntimeError> {
        if self.ty().mutability != Mutability::Var {
            return Err(RuntimeError::new(
                "immutable global cannot be set".to_string(),
            ));
        }
        if val.ty() != self.ty().ty {
            return Err(RuntimeError::new(format!(
                "global of type {:?} cannot be set to {:?}",
                self.ty().ty,
                val.ty()
            )));
        }
        if !val.comes_from_same_store(&self.store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        unsafe {
            let definition = self.global.get_mut();
            match val {
                Val::I32(i) => *definition.as_i32_mut() = i,
                Val::I64(i) => *definition.as_i64_mut() = i,
                Val::F32(f) => *definition.as_f32_mut() = f,
                Val::F64(f) => *definition.as_f64_mut() = f,
                _ => unimplemented!("Global::set for {:?}", val.ty()),
            }
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
