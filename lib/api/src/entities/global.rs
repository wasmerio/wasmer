use crate::{
    error::RuntimeError,
    store::{AsStoreMut, AsStoreRef, StoreMut, StoreRef},
    value::Value,
    vm::{VMExtern, VMExternGlobal},
    ExportError, Exportable, Extern,
};
use wasmer_types::{GlobalType, Mutability};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
#[derive(Debug)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct Global(pub(crate) Box<dyn GlobalLike>);

impl Global {
    /// Create a new global with the initial [`Value`].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    /// assert_eq!(g.ty(&mut store).mutability, Mutability::Const);
    /// ```
    pub fn new(store: &mut impl AsStoreMut, val: Value) -> Self {
        Self::from_value(store, val, Mutability::Const).unwrap()
    }

    /// Create a mutable global with the initial [`Value`].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new_mut(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    /// assert_eq!(g.ty(&mut store).mutability, Mutability::Var);
    /// ```
    pub fn new_mut(store: &mut impl AsStoreMut, val: Value) -> Self {
        Self::from_value(store, val, Mutability::Var).unwrap()
    }

    /// Create a global with the initial [`Value`] and the provided [`Mutability`].
    fn from_value(
        store: &mut impl AsStoreMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        Ok(Self(
            store.as_store_mut().global_from_value(val, mutability)?,
        ))
    }

    /// Returns the [`GlobalType`] of the global.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Type, Value, GlobalType};
    /// # let mut store = Store::default();
    /// #
    /// let c = Global::new(&mut store, Value::I32(1));
    /// let v = Global::new_mut(&mut store, Value::I64(1));
    ///
    /// assert_eq!(c.ty(&mut store), GlobalType::new(Type::I32, Mutability::Const));
    /// assert_eq!(v.ty(&mut store), GlobalType::new(Type::I64, Mutability::Var));
    /// ```
    pub fn ty(&self, store: &impl AsStoreRef) -> GlobalType {
        self.0.ty(store.as_store_ref())
    }

    /// Retrieves the current value [`Value`] that the global has.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    /// ```
    pub fn get(&self, store: &mut impl AsStoreMut) -> Value {
        self.0.get(store.as_store_mut())
    }

    /// Sets a custom [`Value`] to the runtime global.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new_mut(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    ///
    /// g.set(&mut store, Value::I32(2));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(2));
    /// ```
    ///
    /// # Errors
    ///
    /// Trying to mutate a immutable global will raise an error:
    ///
    /// ```should_panic
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// g.set(&mut store, Value::I32(2)).unwrap();
    /// ```
    ///
    /// Trying to set a value of a incompatible type will raise an error:
    ///
    /// ```should_panic
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// // This results in an error: `RuntimeError`.
    /// g.set(&mut store, Value::I64(2)).unwrap();
    /// ```
    pub fn set(&self, store: &mut impl AsStoreMut, val: Value) -> Result<(), RuntimeError> {
        self.0.set(store.as_store_mut(), val)
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternGlobal) -> Self {
        Self(store.as_store_mut().global_from_vm_extern(vm_extern))
    }

    /// Checks whether this global can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.0.is_from_store(store.as_store_ref())
    }

    /// Create a [`VMExtern`] from self.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        self.0.to_vm_extern()
    }
}

impl std::cmp::PartialEq for Global {
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

impl std::cmp::Eq for Global {}

impl Clone for Global {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

impl<'a> Exportable<'a> for Global {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

/// The trait that every concrete global must implement.
pub trait GlobalLike: std::fmt::Debug {
    /// Returns the [`GlobalType`] of the global.
    fn ty(&self, store: StoreRef) -> GlobalType;

    /// Retrieves the current [`Value`] that the global has.
    fn get(&self, store: StoreMut) -> Value;

    /// Sets a custom [`Value`] for the global.
    fn set(&self, store: StoreMut, val: Value) -> Result<(), RuntimeError>;

    /// Checks whether this global can be used with the given context.
    fn is_from_store(&self, store: StoreRef) -> bool;

    /// Create a [`VMExtern`] from self.
    fn to_vm_extern(&self) -> VMExtern;

    /// Create a boxed clone of this implementer.
    fn clone_box(&self) -> Box<dyn GlobalLike>;
}

/// The trait implemented by all those that can create new globals.
pub trait GlobalCreator {
    /// Create a `Global` with the initial [`Value`] and the provided [`Mutability`].
    fn global_from_value(
        &mut self,
        val: Value,
        mutability: Mutability,
    ) -> Result<Box<dyn GlobalLike>, RuntimeError>;

    fn global_from_vm_extern(&mut self, vm_extern: VMExternGlobal) -> Box<dyn GlobalLike>;
}
