use crate::sys::exports::{ExportError, Exportable};
use crate::sys::externals::Extern;
use crate::sys::value::Value;
use crate::sys::GlobalType;
use crate::sys::Mutability;
use crate::sys::RuntimeError;
use crate::sys::Store;
use wasmer_vm::{InternalStoreHandle, StoreHandle, VMExtern, VMGlobal};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
#[derive(Debug, Clone)]
pub struct Global {
    handle: StoreHandle<VMGlobal>,
}

impl Global {
    /// Create a new `Global` with the initial value [`Val`].
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
    pub fn new(store: &mut Store, val: Value) -> Self {
        Self::from_value(store, val, Mutability::Const).unwrap()
    }

    /// Create a mutable `Global` with the initial value [`Val`].
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
    pub fn new_mut(store: &mut Store, val: Value) -> Self {
        Self::from_value(store, val, Mutability::Var).unwrap()
    }

    /// Create a `Global` with the initial value [`Val`] and the provided [`Mutability`].
    fn from_value(
        store: &mut Store,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        if !val.is_from_store(store) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        let global = VMGlobal::new(GlobalType {
            mutability,
            ty: val.ty(),
        });
        unsafe {
            global.vmglobal().as_mut().val = val.as_raw(store);
        }

        Ok(Self {
            handle: StoreHandle::new(store.objects_mut(), global),
        })
    }

    /// Returns the [`GlobalType`] of the `Global`.
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
    pub fn ty(&self, store: &Store) -> GlobalType {
        *self.handle.get(store.objects()).ty()
    }

    /// Retrieves the current value [`Val`] that the Global has.
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
    pub fn get(&self, store: &mut Store) -> Value {
        unsafe {
            let raw = self.handle.get(store.objects()).vmglobal().as_ref().val;
            let ty = self.handle.get(store.objects()).ty().ty;
            Value::from_raw(store, ty, raw)
        }
    }

    /// Sets a custom value [`Val`] to the runtime Global.
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
    pub fn set(&self, store: &mut Store, val: Value) -> Result<(), RuntimeError> {
        if !val.is_from_store(store) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        if self.ty(store).mutability != Mutability::Var {
            return Err(RuntimeError::new("Attempted to set an immutable global"));
        }
        if val.ty() != self.ty(store).ty {
            return Err(RuntimeError::new(format!(
                "Attempted to operate on a global of type {expected} as a global of type {found}",
                expected = self.ty(store).ty,
                found = val.ty(),
            )));
        }
        unsafe {
            self.handle
                .get_mut(store.objects_mut())
                .vmglobal()
                .as_mut()
                .val = val.as_raw(store);
        }
        Ok(())
    }

    pub(crate) fn from_vm_extern(
        store: &mut Store,
        internal: InternalStoreHandle<VMGlobal>,
    ) -> Self {
        Self {
            handle: unsafe { StoreHandle::from_internal(store.objects().id(), internal) },
        }
    }

    /// Checks whether this `Global` can be used with the given store.
    pub fn is_from_store(&self, store: &Store) -> bool {
        self.handle.store_id() == store.objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Global(self.handle.internal_handle())
    }
}

impl std::cmp::PartialEq for Global {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl std::cmp::Eq for Global {}

impl<'a> Exportable<'a> for Global {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
