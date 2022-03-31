use crate::wasm::export::Export;
use crate::wasm::export::VMGlobal;
use crate::wasm::exports::{ExportError, Exportable};
use crate::wasm::externals::Extern;
use crate::wasm::store::Store;
use crate::wasm::types::{Val, ValType};
use crate::wasm::GlobalType;
use crate::wasm::Mutability;
use crate::wasm::RuntimeError;

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
#[derive(Debug, Clone)]
pub struct Global {
    store: Store,
    vm_global: VMGlobal,
}

impl Global {
    /// Create a new `Global` with the initial value [`Val`].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new(&store, Value::I32(1));
    ///
    /// assert_eq!(g.get(), Value::I32(1));
    /// assert_eq!(g.ty().mutability, Mutability::Const);
    /// ```
    pub fn new(store: &Store, val: Val) -> Self {
        Self::from_value(store, val, Mutability::Const).unwrap()
    }

    /// Create a mutable `Global` with the initial value [`Val`].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new_mut(&store, Value::I32(1));
    ///
    /// assert_eq!(g.get(), Value::I32(1));
    /// assert_eq!(g.ty().mutability, Mutability::Var);
    /// ```
    pub fn new_mut(store: &Store, val: Val) -> Self {
        Self::from_value(store, val, Mutability::Var).unwrap()
    }

    /// Create a `Global` with the initial value [`Val`] and the provided [`Mutability`].
    fn from_value(
        _store: &Store,
        _val: Val,
        _mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        panic!("Not implemented!")
    }

    /// Returns the [`GlobalType`] of the `Global`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Type, Value, GlobalType};
    /// # let store = Store::default();
    /// #
    /// let c = Global::new(&store, Value::I32(1));
    /// let v = Global::new_mut(&store, Value::I64(1));
    ///
    /// assert_eq!(c.ty(), &GlobalType::new(Type::I32, Mutability::Const));
    /// assert_eq!(v.ty(), &GlobalType::new(Type::I64, Mutability::Var));
    /// ```
    pub fn ty(&self) -> &GlobalType {
        panic!("Not implemented!")
    }

    /// Returns the [`Store`] where the `Global` belongs.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new(&store, Value::I32(1));
    ///
    /// assert_eq!(g.store(), &store);
    /// ```
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Retrieves the current value [`Val`] that the Global has.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new(&store, Value::I32(1));
    ///
    /// assert_eq!(g.get(), Value::I32(1));
    /// ```
    pub fn get(&self) -> Val {
        panic!("Not implemented!")
    }

    /// Sets a custom value [`Val`] to the runtime Global.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new_mut(&store, Value::I32(1));
    ///
    /// assert_eq!(g.get(), Value::I32(1));
    ///
    /// g.set(Value::I32(2));
    ///
    /// assert_eq!(g.get(), Value::I32(2));
    /// ```
    ///
    /// # Errors
    ///
    /// Trying to mutate a immutable global will raise an error:
    ///
    /// ```should_panic
    /// # use wasmer::{Global, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new(&store, Value::I32(1));
    ///
    /// g.set(Value::I32(2)).unwrap();
    /// ```
    ///
    /// Trying to set a value of a incompatible type will raise an error:
    ///
    /// ```should_panic
    /// # use wasmer::{Global, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new(&store, Value::I32(1));
    ///
    /// // This results in an error: `RuntimeError`.
    /// g.set(Value::I64(2)).unwrap();
    /// ```
    pub fn set(&self, val: Val) -> Result<(), RuntimeError> {
        panic!("Not implemented!")
    }

    pub(crate) fn from_vm_export(store: &Store, vm_global: VMGlobal) -> Self {
        Self {
            store: store.clone(),
            vm_global,
        }
    }

    /// Returns whether or not these two globals refer to the same data.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let store = Store::default();
    /// #
    /// let g = Global::new(&store, Value::I32(1));
    ///
    /// assert!(g.same(&g));
    /// ```
    pub fn same(&self, other: &Self) -> bool {
        panic!("Not implemented!")
    }
}

impl<'a> Exportable<'a> for Global {
    fn to_export(&self) -> Export {
        Export::Global(self.vm_global.clone())
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
