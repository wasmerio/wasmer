use crate::js::context::{AsContextMut, AsContextRef, ContextHandle, InternalContextHandle};
use crate::js::export::VMGlobal;
use crate::js::exports::{ExportError, Exportable};
use crate::js::externals::Extern;
use crate::js::value::Value;
use crate::js::wasm_bindgen_polyfill::Global as JSGlobal;
use crate::js::GlobalType;
use crate::js::Mutability;
use crate::js::RuntimeError;
use wasm_bindgen::JsValue;

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub(crate) handle: ContextHandle<VMGlobal>,
}

impl Global {
    /// Create a new `Global` with the initial value [`Value`].
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

    /// Create a mutable `Global` with the initial value [`Value`].
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

    /// Create a `Global` with the initial value [`Value`] and the provided [`Mutability`].
    fn from_value(
        ctx: &mut impl AsContextMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        if !val.is_from_store(ctx) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        let global_ty = GlobalType {
            mutability,
            ty: val.ty(),
        };
        let descriptor = js_sys::Object::new();
        let (type_str, value) = match val {
            Value::I32(i) => ("i32", JsValue::from_f64(i as _)),
            Value::I64(i) => ("i64", JsValue::from_f64(i as _)),
            Value::F32(f) => ("f32", JsValue::from_f64(f as _)),
            Value::F64(f) => ("f64", JsValue::from_f64(f)),
            _ => unimplemented!("The type is not yet supported in the JS Global API"),
        };
        // This is the value type as string, even though is incorrectly called "value"
        // in the JS API.
        js_sys::Reflect::set(&descriptor, &"value".into(), &type_str.into())?;
        js_sys::Reflect::set(
            &descriptor,
            &"mutable".into(),
            &mutability.is_mutable().into(),
        )?;

        let js_global = JSGlobal::new(&descriptor, &value).unwrap();
        let vm_global = VMGlobal::new(js_global, global_ty);

        Ok(Self::from_vm_export(ctx, vm_global))
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
        self.handle.get(store.objects()).ty
    }

    /// Retrieves the current value [`Value`] that the Global has.
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
    pub fn get(&self, ctx: &impl AsContextRef) -> Value {
        unsafe {
            let raw = self
                .handle
                .get(store.objects())
                .global
                .value()
                .as_f64()
                .unwrap();
            let ty = self.handle.get(store.objects()).ty;
            Value::from_raw(ctx, ty.ty, raw)
        }
        /*
        match self.vm_global.ty.ty {
            ValType::I32 => Value::I32(self.vm_global.global.value().as_f64().unwrap() as _),
            ValType::I64 => Value::I64(self.vm_global.global.value().as_f64().unwrap() as _),
            ValType::F32 => Value::F32(self.vm_global.global.value().as_f64().unwrap() as _),
            ValType::F64 => Value::F64(self.vm_global.global.value().as_f64().unwrap()),
            _ => unimplemented!("The type is not yet supported in the JS Global API"),
        }
        */
    }

    /// Sets a custom value [`Value`] to the runtime Global.
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
        if !val.is_from_store(ctx) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        let global_ty = self.ty(&ctx);
        if global_ty.mutability == Mutability::Const {
            return Err(RuntimeError::new("The global is immutable".to_owned()));
        }
        if val.ty() != global_ty.ty {
            return Err(RuntimeError::new("The types don't match".to_owned()));
        }
        let new_value = match val {
            Value::I32(i) => JsValue::from_f64(i as _),
            Value::I64(i) => JsValue::from_f64(i as _),
            Value::F32(f) => JsValue::from_f64(f as _),
            Value::F64(f) => JsValue::from_f64(f),
            _ => {
                return Err(RuntimeError::new(
                    "The type is not yet supported in the JS Global API".to_owned(),
                ))
            }
        };
        self.handle
            .get_mut(store.objects_mut())
            .global
            .set_value(&new_value);
        Ok(())
    }

    pub(crate) fn from_vm_export(ctx: &mut impl AsContextMut, vm_global: VMGlobal) -> Self {
        Self {
            handle: ContextHandle::new(store.objects_mut(), vm_global),
        }
    }

    pub(crate) fn from_vm_extern(
        ctx: &mut impl AsContextMut,
        internal: InternalContextHandle<VMGlobal>,
    ) -> Self {
        Self {
            handle: unsafe { ContextHandle::from_internal(store.objects().id(), internal) },
        }
    }

    /// Checks whether this `Global` can be used with the given store.
    pub fn is_from_store(&self, ctx: &impl AsContextRef) -> bool {
        self.handle.store_id() == store.objects().id()
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
