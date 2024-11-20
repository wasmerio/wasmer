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
#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub enum RuntimeGlobal {
    #[cfg(feature = "sys")]
    /// The global from the `sys` runtime.
    Sys(crate::rt::sys::global::Global),
    #[cfg(feature = "wamr")]
    /// The global from the `wamr` runtime.
    Wamr(crate::rt::wamr::global::Global),
    #[cfg(feature = "v8")]
    /// The global from the `v8` runtime.
    V8(crate::rt::v8::global::Global),
    #[cfg(feature = "js")]
    /// The global from the `js` runtime.
    Js(crate::rt::js::global::Global),
}

impl RuntimeGlobal {
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
    pub(crate) fn from_value(
        store: &mut impl AsStoreMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::RuntimeStore::Sys(_) => Ok(Self::Sys(
                crate::rt::sys::global::Global::from_value(store, val, mutability)?,
            )),
            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(_) => Ok(Self::Wamr(
                crate::rt::wamr::global::Global::from_value(store, val, mutability)?,
            )),
            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(_) => Ok(Self::V8(crate::rt::v8::global::Global::from_value(
                store, val, mutability,
            )?)),
            #[cfg(feature = "js")]
            crate::RuntimeStore::Js(_) => Ok(Self::Js(crate::rt::js::global::Global::from_value(
                store, val, mutability,
            )?)),
        }
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
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(g) => g.ty(store),
            #[cfg(feature = "wamr")]
            Self::Wamr(g) => g.ty(store),
            #[cfg(feature = "v8")]
            Self::V8(g) => g.ty(store),
            #[cfg(feature = "js")]
            Self::Js(g) => g.ty(store),
        }
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
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(g) => g.get(store),
            #[cfg(feature = "wamr")]
            Self::Wamr(g) => g.get(store),
            #[cfg(feature = "v8")]
            Self::V8(g) => g.get(store),
            #[cfg(feature = "js")]
            Self::Js(g) => g.get(store),
        }
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
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(g) => g.set(store, val),
            #[cfg(feature = "wamr")]
            Self::Wamr(g) => g.set(store, val),
            #[cfg(feature = "v8")]
            Self::V8(g) => g.set(store, val),
            #[cfg(feature = "js")]
            Self::Js(g) => g.set(store, val),
        }
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternGlobal) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::RuntimeStore::Sys(_) => Self::Sys(
                crate::rt::sys::global::Global::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "wamr")]
            crate::RuntimeStore::Wamr(_) => Self::Wamr(
                crate::rt::wamr::global::Global::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "v8")]
            crate::RuntimeStore::V8(_) => Self::V8(crate::rt::v8::global::Global::from_vm_extern(
                store, vm_extern,
            )),
            #[cfg(feature = "js")]
            crate::RuntimeStore::Js(_) => Self::Js(crate::rt::js::global::Global::from_vm_extern(
                store, vm_extern,
            )),
        }
    }

    /// Checks whether this global can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(g) => g.is_from_store(store),
            #[cfg(feature = "wamr")]
            Self::Wamr(g) => g.is_from_store(store),
            #[cfg(feature = "v8")]
            Self::V8(g) => g.is_from_store(store),
            #[cfg(feature = "js")]
            Self::Js(g) => g.is_from_store(store),
        }
    }

    /// Create a [`VMExtern`] from self.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(g) => g.to_vm_extern(),
            #[cfg(feature = "wamr")]
            Self::Wamr(g) => g.to_vm_extern(),
            #[cfg(feature = "v8")]
            Self::V8(g) => g.to_vm_extern(),
            #[cfg(feature = "js")]
            Self::Js(g) => g.to_vm_extern(),
        }
    }
}
