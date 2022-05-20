use crate::sys::context::{AsContextMut, AsContextRef};
use crate::sys::exports::{ExportError, Exportable};
use crate::sys::externals::Extern;
use crate::sys::value::Value;
use crate::sys::GlobalType;
use crate::sys::Mutability;
use crate::sys::RuntimeError;
use wasmer_vm::{ContextHandle, InternalContextHandle, VMExtern, VMGlobal};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
#[derive(Debug, Clone)]
pub struct Global {
    handle: ContextHandle<VMGlobal>,
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
    pub fn new(ctx: &mut impl AsContextMut, val: Value) -> Self {
        Self::from_value(ctx, val, Mutability::Const).unwrap()
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
    pub fn new_mut(ctx: &mut impl AsContextMut, val: Value) -> Self {
        Self::from_value(ctx, val, Mutability::Var).unwrap()
    }

    /// Create a `Global` with the initial value [`Val`] and the provided [`Mutability`].
    fn from_value(
        ctx: &mut impl AsContextMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        if !val.is_from_context(ctx) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        let global = VMGlobal::new(GlobalType {
            mutability,
            ty: val.ty(),
        });
        unsafe {
            global.vmglobal().as_mut().val = val.as_raw(ctx);
        }

        Ok(Self {
            handle: ContextHandle::new(ctx.as_context_mut().objects_mut(), global),
        })
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
    pub fn ty(&self, ctx: &impl AsContextRef) -> GlobalType {
        *self.handle.get(ctx.as_context_ref().objects()).ty()
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
    pub fn get(&self, ctx: &mut impl AsContextMut) -> Value {
        unsafe {
            let raw = self
                .handle
                .get(ctx.as_context_ref().objects())
                .vmglobal()
                .as_ref()
                .val;
            let ty = self.handle.get(ctx.as_context_ref().objects()).ty().ty;
            Value::from_raw(ctx, ty, raw)
        }
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
    pub fn set(&self, ctx: &mut impl AsContextMut, val: Value) -> Result<(), RuntimeError> {
        if !val.is_from_context(ctx) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        if self.ty(ctx).mutability != Mutability::Var {
            return Err(RuntimeError::new("Attempted to set an immutable global"));
        }
        if val.ty() != self.ty(ctx).ty {
            return Err(RuntimeError::new(format!(
                "Attempted to operate on a global of type {expected} as a global of type {found}",
                expected = self.ty(ctx).ty,
                found = val.ty(),
            )));
        }
        unsafe {
            self.handle
                .get_mut(ctx.as_context_mut().objects_mut())
                .vmglobal()
                .as_mut()
                .val = val.as_raw(ctx);
        }
        Ok(())
    }

    pub(crate) fn from_vm_extern(
        ctx: &mut impl AsContextMut,
        internal: InternalContextHandle<VMGlobal>,
    ) -> Self {
        Self {
            handle: unsafe {
                ContextHandle::from_internal(ctx.as_context_ref().objects().id(), internal)
            },
        }
    }

    /// Checks whether this `Global` can be used with the given context.
    pub fn is_from_context(&self, ctx: &impl AsContextRef) -> bool {
        self.handle.context_id() == ctx.as_context_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Global(self.handle.internal_handle())
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
