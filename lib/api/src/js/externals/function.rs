pub use self::inner::{FromToNativeWasmType, HostFunction, WasmTypeList, WithEnv, WithoutEnv};
use crate::js::exports::{ExportError, Exportable};
use crate::js::externals::{Extern, VMExtern};
use crate::js::function_env::FunctionEnvMut;
use crate::js::store::{AsStoreMut, AsStoreRef, InternalStoreHandle, StoreHandle, StoreMut};
use crate::js::types::{param_from_js, AsJs}; /* ValFuncRef */
use crate::js::RuntimeError;
use crate::js::TypedFunction;
use crate::js::Value;
use crate::js::{FunctionEnv, FunctionType};
use js_sys::{Array, Function as JSFunction};
use std::iter::FromIterator;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::js::export::VMFunction;
use std::fmt;

#[repr(C)]
pub struct VMFunctionBody(u8);

#[inline]
fn result_to_js(val: &Value) -> JsValue {
    match val {
        Value::I32(i) => JsValue::from_f64(*i as _),
        Value::I64(i) => JsValue::from_f64(*i as _),
        Value::F32(f) => JsValue::from_f64(*f as _),
        Value::F64(f) => JsValue::from_f64(*f),
        Value::V128(f) => JsValue::from_f64(*f as _),
        val => unimplemented!(
            "The value `{:?}` is not yet supported in the JS Function API",
            val
        ),
    }
}

#[inline]
fn results_to_js_array(values: &[Value]) -> Array {
    Array::from_iter(values.iter().map(result_to_js))
}

/// A WebAssembly `function` instance.
///
/// A function instance is the runtime representation of a function.
/// It effectively is a closure of the original function (defined in either
/// the host or the WebAssembly module) over the runtime `Instance` of its
/// originating `Module`.
///
/// The module instance is used to resolve references to other definitions
/// during execution of the function.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#function-instances>
///
/// # Panics
/// - Closures (functions with captured environments) are not currently supported
///   with native functions. Attempting to create a native `Function` with one will
///   result in a panic.
///   [Closures as host functions tracking issue](https://github.com/wasmerio/wasmer/issues/1840)
#[derive(Clone, PartialEq)]
pub struct Function {
    pub(crate) handle: StoreHandle<VMFunction>,
}

impl Function {
    /// Creates a new host `Function` (dynamic) with the provided signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Function::new_typed`] for less runtime overhead.
    pub fn new<FT, F>(store: &mut impl AsStoreMut, ty: FT, func: F) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(&[Value]) -> Result<Vec<Value>, RuntimeError> + 'static + Send + Sync,
    {
        let env = FunctionEnv::new(&mut store.as_store_mut(), ());
        let wrapped_func = move |_env: FunctionEnvMut<()>,
                                 args: &[Value]|
              -> Result<Vec<Value>, RuntimeError> { func(args) };
        Self::new_with_env(store, &env, ty, wrapped_func)
    }

    /// To `VMExtern`.
    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Function(self.handle.internal_handle())
    }

    /// Creates a new host `Function` (dynamic) with the provided signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Function::new_typed`] or [`Function::new_typed_with_env`]
    /// for less runtime overhead.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{Function, FunctionType, Type, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let signature = FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32]);
    ///
    /// let f = Function::new_with_env(&store, &signature, |args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    ///
    /// With constant signature:
    ///
    /// ```
    /// # use wasmer::{Function, FunctionType, Type, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// const I32_I32_TO_I32: ([Type; 2], [Type; 1]) = ([Type::I32, Type::I32], [Type::I32]);
    ///
    /// let f = Function::new_with_env(&store, I32_I32_TO_I32, |args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    #[allow(clippy::cast_ptr_alignment)]
    pub fn new_with_env<FT, F, T: Send + 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(FunctionEnvMut<'_, T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        let mut store = store.as_store_mut();
        let function_type = ty.into();
        let func_ty = function_type.clone();
        let raw_store = store.as_raw() as *mut u8;
        let raw_env = env.clone();
        let wrapped_func: JsValue = match function_type.results().len() {
            0 => Closure::wrap(Box::new(move |args: &Array| {
                let mut store: StoreMut = unsafe { StoreMut::from_raw(raw_store as _) };
                let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
                let wasm_arguments = function_type
                    .params()
                    .iter()
                    .enumerate()
                    .map(|(i, param)| param_from_js(param, &args.get(i as u32)))
                    .collect::<Vec<_>>();
                let _results = func(env, &wasm_arguments)?;
                Ok(())
            })
                as Box<dyn FnMut(&Array) -> Result<(), JsValue>>)
            .into_js_value(),
            1 => Closure::wrap(Box::new(move |args: &Array| {
                let mut store: StoreMut = unsafe { StoreMut::from_raw(raw_store as _) };
                let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
                let wasm_arguments = function_type
                    .params()
                    .iter()
                    .enumerate()
                    .map(|(i, param)| param_from_js(param, &args.get(i as u32)))
                    .collect::<Vec<_>>();
                let results = func(env, &wasm_arguments)?;
                return Ok(result_to_js(&results[0]));
            })
                as Box<dyn FnMut(&Array) -> Result<JsValue, JsValue>>)
            .into_js_value(),
            _n => Closure::wrap(Box::new(move |args: &Array| {
                let mut store: StoreMut = unsafe { StoreMut::from_raw(raw_store as _) };
                let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
                let wasm_arguments = function_type
                    .params()
                    .iter()
                    .enumerate()
                    .map(|(i, param)| param_from_js(param, &args.get(i as u32)))
                    .collect::<Vec<_>>();
                let results = func(env, &wasm_arguments)?;
                return Ok(results_to_js_array(&results));
            })
                as Box<dyn FnMut(&Array) -> Result<Array, JsValue>>)
            .into_js_value(),
        };

        let dyn_func =
            JSFunction::new_with_args("f", "return f(Array.prototype.slice.call(arguments, 1))");
        let binded_func = dyn_func.bind1(&JsValue::UNDEFINED, &wrapped_func);
        let vm_function = VMFunction::new(binded_func, func_ty);
        Self::from_vm_export(&mut store, vm_function)
    }

    #[deprecated(
        since = "3.0.0",
        note = "new_native() has been renamed to new_typed()."
    )]
    /// Creates a new host `Function` from a native function.
    pub fn new_native<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        Self::new_typed(store, func)
    }

    /// Creates a new host `Function` from a native function.
    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let mut store = store.as_store_mut();
        if std::mem::size_of::<F>() != 0 {
            Self::closures_unsupported_panic();
        }
        let function = inner::Function::<Args, Rets>::new(func);
        let address = function.address() as usize as u32;

        let ft = wasm_bindgen::function_table();
        let as_table = ft.unchecked_ref::<js_sys::WebAssembly::Table>();
        let func = as_table.get(address).unwrap();

        let binded_func = func.bind1(
            &JsValue::UNDEFINED,
            &JsValue::from_f64(store.as_raw() as *mut u8 as usize as f64),
        );
        let ty = function.ty();
        let vm_function = VMFunction::new(binded_func, ty);
        Self {
            handle: StoreHandle::new(store.objects_mut(), vm_function),
        }
    }

    #[deprecated(
        since = "3.0.0",
        note = "new_native_with_env() has been renamed to new_typed_with_env()."
    )]
    /// Creates a new host `Function` with an environment from a typed function.
    pub fn new_native_with_env<T: Send + 'static, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        F: HostFunction<T, Args, Rets, WithEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        Self::new_typed_with_env(store, env, func)
    }

    /// Creates a new host `Function` from a typed function.
    ///
    /// The function signature is automatically retrieved using the
    /// Rust typing system.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Store, Function};
    /// # let mut store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&store, sum);
    /// ```
    pub fn new_typed_with_env<T, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        F: HostFunction<T, Args, Rets, WithEnv>,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let mut store = store.as_store_mut();
        if std::mem::size_of::<F>() != 0 {
            Self::closures_unsupported_panic();
        }
        let function = inner::Function::<Args, Rets>::new(func);
        let address = function.address() as usize as u32;

        let ft = wasm_bindgen::function_table();
        let as_table = ft.unchecked_ref::<js_sys::WebAssembly::Table>();
        let func = as_table.get(address).unwrap();

        let binded_func = func.bind2(
            &JsValue::UNDEFINED,
            &JsValue::from_f64(store.as_raw() as *mut u8 as usize as f64),
            &JsValue::from_f64(env.handle.internal_handle().index() as f64),
        );
        let ty = function.ty();
        let vm_function = VMFunction::new(binded_func, ty);
        Self {
            handle: StoreHandle::new(store.objects_mut(), vm_function),
        }
    }

    /// Returns the [`FunctionType`] of the `Function`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, Store, Type};
    /// # let mut store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed(&store, sum);
    ///
    /// assert_eq!(f.ty().params(), vec![Type::I32, Type::I32]);
    /// assert_eq!(f.ty().results(), vec![Type::I32]);
    /// ```
    pub fn ty(&self, store: &impl AsStoreRef) -> FunctionType {
        self.handle.get(store.as_store_ref().objects()).ty.clone()
    }

    /// Returns the number of parameters that this function takes.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionEnvMut, Store, Type};
    /// # let mut store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed(&store, sum);
    ///
    /// assert_eq!(f.param_arity(&store), 2);
    /// ```
    pub fn param_arity(&self, store: &impl AsStoreRef) -> usize {
        self.ty(store).params().len()
    }

    /// Returns the number of results this function produces.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionEnvMut, Store, Type};
    /// # let mut store = Store::default();
    /// #
    /// fn sum(a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed(&store, sum);
    ///
    /// assert_eq!(f.result_arity(&store), 1);
    /// ```
    pub fn result_arity(&self, store: &impl AsStoreRef) -> usize {
        self.ty(store).results().len()
    }

    /// Call the `Function` function.
    ///
    /// Depending on where the Function is defined, it will call it.
    /// 1. If the function is defined inside a WebAssembly, it will call the trampoline
    ///    for the function signature.
    /// 2. If the function is defined in the host (in a native way), it will
    ///    call the trampoline.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let mut store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// assert_eq!(sum.call(&[Value::I32(1), Value::I32(2)]).unwrap().to_vec(), vec![Value::I32(3)]);
    /// ```
    pub fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        let arr = js_sys::Array::new_with_length(params.len() as u32);

        // let raw_env = env.as_raw() as *mut u8;
        // let mut env = unsafe { FunctionEnvMut::from_raw(raw_env as *mut StoreInner<()>) };

        for (i, param) in params.iter().enumerate() {
            let js_value = param.as_jsvalue(&store.as_store_ref());
            arr.set(i as u32, js_value);
        }
        let result = js_sys::Reflect::apply(
            &self.handle.get(store.as_store_ref().objects()).function,
            &wasm_bindgen::JsValue::NULL,
            &arr,
        )?;

        let result_types = self.handle.get(store.as_store_ref().objects()).ty.results();
        match result_types.len() {
            0 => Ok(Box::new([])),
            1 => {
                let value = param_from_js(&result_types[0], &result);
                Ok(vec![value].into_boxed_slice())
            }
            _n => {
                let result_array: Array = result.into();
                Ok(result_array
                    .iter()
                    .enumerate()
                    .map(|(i, js_val)| param_from_js(&result_types[i], &js_val))
                    .collect::<Vec<_>>()
                    .into_boxed_slice())
            }
        }
    }

    pub(crate) fn from_vm_export(store: &mut impl AsStoreMut, vm_function: VMFunction) -> Self {
        Self {
            handle: StoreHandle::new(store.objects_mut(), vm_function),
        }
    }

    pub(crate) fn from_vm_extern(
        store: &mut impl AsStoreMut,
        internal: InternalStoreHandle<VMFunction>,
    ) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(store.as_store_ref().objects().id(), internal)
            },
        }
    }

    #[deprecated(since = "3.0.0", note = "native() has been renamed to typed().")]
    /// Transform this WebAssembly function into a typed function.
    pub fn native<Args, Rets>(
        &self,
        store: &impl AsStoreRef,
    ) -> Result<TypedFunction<Args, Rets>, RuntimeError>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        self.typed(store)
    }

    /// Transform this WebAssembly function into a typed function.
    /// See [`TypedFunction`] to learn more.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let mut store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    /// let sum_typed = sum.typed::<(i32, i32), i32>().unwrap();
    ///
    /// assert_eq!(sum_typed.call(&mut store, 1, 2).unwrap(), 3);
    /// ```
    ///
    /// # Errors
    ///
    /// If the `Args` generic parameter does not match the exported function
    /// an error will be raised:
    ///
    /// ```should_panic
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let mut store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_typed = sum.typed::<(i64, i64), i32>(&mut store).unwrap();
    /// ```
    ///
    /// If the `Rets` generic parameter does not match the exported function
    /// an error will be raised:
    ///
    /// ```should_panic
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, Value};
    /// # let mut store = Store::default();
    /// # let wasm_bytes = wat2wasm(r#"
    /// # (module
    /// #   (func (export "sum") (param $x i32) (param $y i32) (result i32)
    /// #     local.get $x
    /// #     local.get $y
    /// #     i32.add
    /// #   ))
    /// # "#.as_bytes()).unwrap();
    /// # let module = Module::new(&store, wasm_bytes).unwrap();
    /// # let import_object = imports! {};
    /// # let instance = Instance::new(&module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_typed = sum.typed::<(i32, i32), i64>(&mut store).unwrap();
    /// ```
    pub fn typed<Args, Rets>(
        &self,
        store: &impl AsStoreRef,
    ) -> Result<TypedFunction<Args, Rets>, RuntimeError>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let ty = self.ty(store);
        // type check
        {
            let expected = ty.params();
            let given = Args::wasm_types();

            if expected != given {
                return Err(RuntimeError::new(format!(
                    "given types (`{:?}`) for the function arguments don't match the actual types (`{:?}`)",
                    given,
                    expected,
                )));
            }
        }

        {
            let expected = ty.results();
            let given = Rets::wasm_types();

            if expected != given {
                // todo: error result types don't match
                return Err(RuntimeError::new(format!(
                    "given types (`{:?}`) for the function results don't match the actual types (`{:?}`)",
                    given,
                    expected,
                )));
            }
        }

        Ok(TypedFunction::from_handle(self.clone()))
    }

    #[track_caller]
    fn closures_unsupported_panic() -> ! {
        unimplemented!("Closures (functions with captured environments) are currently unsupported with native functions. See: https://github.com/wasmerio/wasmer/issues/1840")
    }

    /// Checks whether this `Function` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }
}

impl<'a> Exportable<'a> for Function {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Function(func) => Ok(func),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.debug_struct("Function").finish()
    }
}

/// This private inner module contains the low-level implementation
/// for `Function` and its siblings.
mod inner {
    use super::RuntimeError;
    use super::VMFunctionBody;
    use crate::js::function_env::{FunctionEnvMut, VMFunctionEnvironment};
    use crate::js::store::{AsStoreMut, AsStoreRef, InternalStoreHandle, StoreHandle, StoreMut};
    use crate::js::FunctionEnv;
    use crate::js::NativeWasmTypeInto;
    use std::array::TryFromSliceError;
    use std::convert::{Infallible, TryInto};
    use std::error::Error;
    use std::marker::PhantomData;
    use std::panic::{self, AssertUnwindSafe};

    use wasmer_types::{FunctionType, NativeWasmType, Type};
    // use wasmer::{raise_user_trap, resume_panic};

    /// A trait to convert a Rust value to a `WasmNativeType` value,
    /// or to convert `WasmNativeType` value to a Rust value.
    ///
    /// This trait should ideally be split into two traits:
    /// `FromNativeWasmType` and `ToNativeWasmType` but it creates a
    /// non-negligible complexity in the `WasmTypeList`
    /// implementation.
    pub unsafe trait FromToNativeWasmType
    where
        Self: Sized,
    {
        /// Native Wasm type.
        type Native: NativeWasmTypeInto;

        /// Convert a value of kind `Self::Native` to `Self`.
        ///
        /// # Panics
        ///
        /// This method panics if `native` cannot fit in the `Self`
        /// type`.
        fn from_native(native: Self::Native) -> Self;

        /// Convert self to `Self::Native`.
        ///
        /// # Panics
        ///
        /// This method panics if `self` cannot fit in the
        /// `Self::Native` type.
        fn to_native(self) -> Self::Native;

        /// Returns whether this native type belongs to the given store
        fn is_from_store(&self, _store: &impl AsStoreRef) -> bool;
    }

    macro_rules! from_to_native_wasm_type {
        ( $( $type:ty => $native_type:ty ),* ) => {
            $(
                #[allow(clippy::use_self)]
                unsafe impl FromToNativeWasmType for $type {
                    type Native = $native_type;

                    #[inline]
                    fn from_native(native: Self::Native) -> Self {
                        native as Self
                    }

                    #[inline]
                    fn to_native(self) -> Self::Native {
                        self as Self::Native
                    }

                    #[inline]
                    fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
                        true // Javascript only has one store
                    }
                }
            )*
        };
    }

    macro_rules! from_to_native_wasm_type_same_size {
        ( $( $type:ty => $native_type:ty ),* ) => {
            $(
                #[allow(clippy::use_self)]
                unsafe impl FromToNativeWasmType for $type {
                    type Native = $native_type;

                    #[inline]
                    fn from_native(native: Self::Native) -> Self {
                        Self::from_ne_bytes(Self::Native::to_ne_bytes(native))
                    }

                    #[inline]
                    fn to_native(self) -> Self::Native {
                        Self::Native::from_ne_bytes(Self::to_ne_bytes(self))
                    }

                    #[inline]
                    fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
                        true // Javascript only has one store
                    }
                }
            )*
        };
    }

    from_to_native_wasm_type!(
        i8 => i32,
        u8 => i32,
        i16 => i32,
        u16 => i32
    );

    from_to_native_wasm_type_same_size!(
        i32 => i32,
        u32 => i32,
        i64 => i64,
        u64 => i64,
        f32 => f32,
        f64 => f64
    );

    #[cfg(test)]
    mod test_from_to_native_wasm_type {
        use super::FromToNativeWasmType;

        #[test]
        fn test_to_native() {
            assert_eq!(7i8.to_native(), 7i32);
            assert_eq!(7u8.to_native(), 7i32);
            assert_eq!(7i16.to_native(), 7i32);
            assert_eq!(7u16.to_native(), 7i32);
            assert_eq!(u32::MAX.to_native(), -1);
        }

        #[test]
        fn test_to_native_same_size() {
            assert_eq!(7i32.to_native(), 7i32);
            assert_eq!(7u32.to_native(), 7i32);
            assert_eq!(7i64.to_native(), 7i64);
            assert_eq!(7u64.to_native(), 7i64);
            assert_eq!(7f32.to_native(), 7f32);
            assert_eq!(7f64.to_native(), 7f64);
        }
    }

    /// The `WasmTypeList` trait represents a tuple (list) of Wasm
    /// typed values. It is used to get low-level representation of
    /// such a tuple.
    pub trait WasmTypeList
    where
        Self: Sized,
    {
        /// The C type (a struct) that can hold/represent all the
        /// represented values.
        type CStruct;

        /// The array type that can hold all the represented values.
        ///
        /// Note that all values are stored in their binary form.
        type Array: AsMut<[f64]>;

        /// The size of the array
        fn size() -> u32;

        /// Constructs `Self` based on an array of values.
        ///
        /// # Safety
        unsafe fn from_array(store: &mut impl AsStoreMut, array: Self::Array) -> Self;

        /// Constructs `Self` based on a slice of values.
        ///
        /// `from_slice` returns a `Result` because it is possible
        /// that the slice doesn't have the same size than
        /// `Self::Array`, in which circumstance an error of kind
        /// `TryFromSliceError` will be returned.
        ///
        /// # Safety
        unsafe fn from_slice(
            store: &mut impl AsStoreMut,
            slice: &[f64],
        ) -> Result<Self, TryFromSliceError>;

        /// Builds and returns an array of type `Array` from a tuple
        /// (list) of values.
        ///
        /// # Safety
        unsafe fn into_array(self, store: &mut impl AsStoreMut) -> Self::Array;

        /// Allocates and return an empty array of type `Array` that
        /// will hold a tuple (list) of values, usually to hold the
        /// returned values of a WebAssembly function call.
        fn empty_array() -> Self::Array;

        /// Builds a tuple (list) of values from a C struct of type
        /// `CStruct`.
        ///
        /// # Safety
        unsafe fn from_c_struct(store: &mut impl AsStoreMut, c_struct: Self::CStruct) -> Self;

        /// Builds and returns a C struct of type `CStruct` from a
        /// tuple (list) of values.
        ///
        /// # Safety
        unsafe fn into_c_struct(self, store: &mut impl AsStoreMut) -> Self::CStruct;

        /// Writes the contents of a C struct to an array of `f64`.
        ///
        /// # Safety
        unsafe fn write_c_struct_to_ptr(c_struct: Self::CStruct, ptr: *mut f64);

        /// Get the Wasm types for the tuple (list) of currently
        /// represented values.
        fn wasm_types() -> &'static [Type];
    }

    /// The `IntoResult` trait turns a `WasmTypeList` into a
    /// `Result<WasmTypeList, Self::Error>`.
    ///
    /// It is mostly used to turn result values of a Wasm function
    /// call into a `Result`.
    pub trait IntoResult<T>
    where
        T: WasmTypeList,
    {
        /// The error type for this trait.
        type Error: Error + Sync + Send + 'static;

        /// Transforms `Self` into a `Result`.
        fn into_result(self) -> Result<T, Self::Error>;
    }

    impl<T> IntoResult<T> for T
    where
        T: WasmTypeList,
    {
        // `T` is not a `Result`, it's already a value, so no error
        // can be built.
        type Error = Infallible;

        fn into_result(self) -> Result<Self, Infallible> {
            Ok(self)
        }
    }

    impl<T, E> IntoResult<T> for Result<T, E>
    where
        T: WasmTypeList,
        E: Error + Sync + Send + 'static,
    {
        type Error = E;

        fn into_result(self) -> Self {
            self
        }
    }

    #[cfg(test)]
    mod test_into_result {
        use super::*;
        use std::convert::Infallible;

        #[test]
        fn test_into_result_over_t() {
            let x: i32 = 42;
            let result_of_x: Result<i32, Infallible> = x.into_result();

            assert_eq!(result_of_x.unwrap(), x);
        }

        #[test]
        fn test_into_result_over_result() {
            {
                let x: Result<i32, Infallible> = Ok(42);
                let result_of_x: Result<i32, Infallible> = x.into_result();

                assert_eq!(result_of_x, x);
            }

            {
                use std::{error, fmt};

                #[derive(Debug, PartialEq)]
                struct E;

                impl fmt::Display for E {
                    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(formatter, "E")
                    }
                }

                impl error::Error for E {}

                let x: Result<Infallible, E> = Err(E);
                let result_of_x: Result<Infallible, E> = x.into_result();

                assert_eq!(result_of_x.unwrap_err(), E);
            }
        }
    }

    /// The `HostFunction` trait represents the set of functions that
    /// can be used as host function. To uphold this statement, it is
    /// necessary for a function to be transformed into a pointer to
    /// `VMFunctionBody`.
    pub trait HostFunction<T, Args, Rets, Kind>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
        Kind: HostFunctionKind,
    {
        /// Get the pointer to the function body.
        fn function_body_ptr(&self) -> *const VMFunctionBody;

        // /// Get the pointer to the function call trampoline.
        // fn call_trampoline_address() -> VMTrampoline;
    }

    /// Empty trait to specify the kind of `HostFunction`: With or
    /// without an environment.
    ///
    /// This trait is never aimed to be used by a user. It is used by
    /// the trait system to automatically generate the appropriate
    /// host functions.
    #[doc(hidden)]
    pub trait HostFunctionKind: private::HostFunctionKindSealed {}

    /// An empty struct to help Rust typing to determine
    /// when a `HostFunction` does have an environment.
    pub struct WithEnv;

    impl HostFunctionKind for WithEnv {}

    /// An empty struct to help Rust typing to determine
    /// when a `HostFunction` does not have an environment.
    pub struct WithoutEnv;

    impl HostFunctionKind for WithoutEnv {}

    mod private {
        //! Sealing the HostFunctionKind because it shouldn't be implemented
        //! by any type outside.
        //! See:
        //! https://rust-lang.github.io/api-guidelines/future-proofing.html#c-sealed
        pub trait HostFunctionKindSealed {}
        impl HostFunctionKindSealed for super::WithEnv {}
        impl HostFunctionKindSealed for super::WithoutEnv {}
    }

    /// Represents a low-level Wasm static host function. See
    /// `super::Function::new` and `super::Function::new_env` to learn
    /// more.
    #[derive(Clone, Debug, Hash, PartialEq, Eq)]
    pub struct Function<Args = (), Rets = ()> {
        address: *const VMFunctionBody,
        _phantom: PhantomData<(Args, Rets)>,
    }

    unsafe impl<Args, Rets> Send for Function<Args, Rets> {}

    impl<Args, Rets> Function<Args, Rets>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        /// Creates a new `Function`.
        #[allow(dead_code)]
        pub fn new<F, T, Kind: HostFunctionKind>(function: F) -> Self
        where
            F: HostFunction<T, Args, Rets, Kind>,
            T: Sized,
        {
            Self {
                address: function.function_body_ptr(),
                _phantom: PhantomData,
            }
        }

        /// Get the function type of this `Function`.
        #[allow(dead_code)]
        pub fn ty(&self) -> FunctionType {
            FunctionType::new(Args::wasm_types(), Rets::wasm_types())
        }

        /// Get the address of this `Function`.
        #[allow(dead_code)]
        pub fn address(&self) -> *const VMFunctionBody {
            self.address
        }
    }

    macro_rules! impl_host_function {
        ( [$c_struct_representation:ident]
           $c_struct_name:ident,
           $( $x:ident ),* ) => {

            /// A structure with a C-compatible representation that can hold a set of Wasm values.
            /// This type is used by `WasmTypeList::CStruct`.
            #[repr($c_struct_representation)]
            pub struct $c_struct_name< $( $x ),* > ( $( <<$x as FromToNativeWasmType>::Native as NativeWasmType>::Abi ),* )
            where
                $( $x: FromToNativeWasmType ),*;

            // Implement `WasmTypeList` for a specific tuple.
            #[allow(unused_parens, dead_code)]
            impl< $( $x ),* >
                WasmTypeList
            for
                ( $( $x ),* )
            where
                $( $x: FromToNativeWasmType ),*
            {
                type CStruct = $c_struct_name< $( $x ),* >;

                type Array = [f64; count_idents!( $( $x ),* )];

                fn size() -> u32 {
                    count_idents!( $( $x ),* ) as _
                }

                #[allow(unused_mut)]
                #[allow(clippy::unused_unit)]
                #[allow(clippy::missing_safety_doc)]
                unsafe fn from_array(mut _store: &mut impl AsStoreMut, array: Self::Array) -> Self {
                    // Unpack items of the array.
                    #[allow(non_snake_case)]
                    let [ $( $x ),* ] = array;

                    // Build the tuple.
                    (
                        $(
                            FromToNativeWasmType::from_native(NativeWasmTypeInto::from_raw(_store, $x))
                        ),*
                    )
                }

                #[allow(clippy::missing_safety_doc)]
                unsafe fn from_slice(store: &mut impl AsStoreMut, slice: &[f64]) -> Result<Self, TryFromSliceError> {
                    Ok(Self::from_array(store, slice.try_into()?))
                }

                #[allow(unused_mut)]
                #[allow(clippy::missing_safety_doc)]
                unsafe fn into_array(self, mut _store: &mut impl AsStoreMut) -> Self::Array {
                    // Unpack items of the tuple.
                    #[allow(non_snake_case)]
                    let ( $( $x ),* ) = self;

                    // Build the array.
                    [
                        $(
                            FromToNativeWasmType::to_native($x).into_raw(_store)
                        ),*
                    ]
                }

                fn empty_array() -> Self::Array {
                    // Build an array initialized with `0`.
                    [0_f64; count_idents!( $( $x ),* )]
                }

                #[allow(unused_mut)]
                #[allow(clippy::unused_unit)]
                #[allow(clippy::missing_safety_doc)]
                unsafe fn from_c_struct(mut _store: &mut impl AsStoreMut, c_struct: Self::CStruct) -> Self {
                    // Unpack items of the C structure.
                    #[allow(non_snake_case)]
                    let $c_struct_name( $( $x ),* ) = c_struct;

                    (
                        $(
                            FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(_store, $x))
                        ),*
                    )
                }

                #[allow(unused_parens, non_snake_case, unused_mut)]
                #[allow(clippy::missing_safety_doc)]
                unsafe fn into_c_struct(self, mut _store: &mut impl AsStoreMut) -> Self::CStruct {
                    // Unpack items of the tuple.
                    let ( $( $x ),* ) = self;

                    // Build the C structure.
                    $c_struct_name(
                        $(
                            FromToNativeWasmType::to_native($x).into_abi(_store)
                        ),*
                    )
                }

                #[allow(non_snake_case)]
                unsafe fn write_c_struct_to_ptr(c_struct: Self::CStruct, _ptr: *mut f64) {
                    // Unpack items of the tuple.
                    let $c_struct_name( $( $x ),* ) = c_struct;

                    let mut _n = 0;
                    $(
                        *_ptr.add(_n).cast() = $x;
                        _n += 1;
                    )*
                }

                fn wasm_types() -> &'static [Type] {
                    &[
                        $(
                            $x::Native::WASM_TYPE
                        ),*
                    ]
                }
            }

            // Implement `HostFunction` for a function with a [`FunctionEnvMut`] that has the same
            // arity than the tuple.
            #[allow(unused_parens)]
            impl< $( $x, )* Rets, RetsAsResult, T, Func >
                HostFunction<T, ( $( $x ),* ), Rets, WithEnv>
            for
                Func
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                T: Send + 'static,
                Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
            {
                #[allow(non_snake_case)]
                fn function_body_ptr(&self) -> *const VMFunctionBody {
                    /// This is a function that wraps the real host
                    /// function. Its address will be used inside the
                    /// runtime.
                    unsafe extern "C" fn func_wrapper<T, $( $x, )* Rets, RetsAsResult, Func>( store_ptr: usize, handle_index: usize, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
                    where
                        $( $x: FromToNativeWasmType, )*
                        Rets: WasmTypeList,
                        RetsAsResult: IntoResult<Rets>,
                        T: Send + 'static,
                        Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
                    {
                        // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
                        let func: &Func = &*(&() as *const () as *const Func);
                        let mut store = StoreMut::from_raw(store_ptr as *mut _);
                        let mut store2 = StoreMut::from_raw(store_ptr as *mut _);

                        let result = panic::catch_unwind(AssertUnwindSafe(|| {
                            let handle: StoreHandle<VMFunctionEnvironment> = StoreHandle::from_internal(store2.objects_mut().id(), InternalStoreHandle::from_index(handle_index).unwrap());
                            let env: FunctionEnvMut<T> = FunctionEnv::from_handle(handle).into_mut(&mut store2);
                            func(env, $( FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x)) ),* ).into_result()
                        }));

                        match result {
                            Ok(Ok(result)) => return result.into_c_struct(&mut store),
                            #[allow(deprecated)]
                            #[cfg(feature = "std")]
                            Ok(Err(trap)) => RuntimeError::raise(Box::new(trap)),
                            #[cfg(feature = "core")]
                            #[allow(deprecated)]
                            Ok(Err(trap)) => RuntimeError::raise(Box::new(trap)),
                            Err(_panic) => unimplemented!(),
                        }
                    }

                    func_wrapper::< T, $( $x, )* Rets, RetsAsResult, Self > as *const VMFunctionBody
                }
            }

            // Implement `HostFunction` for a function that has the same arity than the tuple.
            #[allow(unused_parens)]
            impl< $( $x, )* Rets, RetsAsResult, Func >
                HostFunction<(), ( $( $x ),* ), Rets, WithoutEnv>
            for
                Func
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn($( $x , )*) -> RetsAsResult + 'static,
            {
                #[allow(non_snake_case)]
                fn function_body_ptr(&self) -> *const VMFunctionBody {
                    /// This is a function that wraps the real host
                    /// function. Its address will be used inside the
                    /// runtime.
                    unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func>( store_ptr: usize, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
                    where
                        $( $x: FromToNativeWasmType, )*
                        Rets: WasmTypeList,
                        RetsAsResult: IntoResult<Rets>,
                        Func: Fn($( $x , )*) -> RetsAsResult + 'static,
                    {
                        // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
                        let func: &Func = &*(&() as *const () as *const Func);
                        let mut store = StoreMut::from_raw(store_ptr as *mut _);

                        let result = panic::catch_unwind(AssertUnwindSafe(|| {
                            func($( FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x)) ),* ).into_result()
                        }));

                        match result {
                            Ok(Ok(result)) => return result.into_c_struct(&mut store),
                            #[cfg(feature = "std")]
                            #[allow(deprecated)]
                            Ok(Err(trap)) => RuntimeError::raise(Box::new(trap)),
                            #[cfg(feature = "core")]
                            #[allow(deprecated)]
                            Ok(Err(trap)) => RuntimeError::raise(Box::new(trap)),
                            Err(_panic) => unimplemented!(),
                        }
                    }

                    func_wrapper::< $( $x, )* Rets, RetsAsResult, Self > as *const VMFunctionBody
                }
            }
        };
    }

    // Black-magic to count the number of identifiers at compile-time.
    macro_rules! count_idents {
        ( $($idents:ident),* ) => {
            {
                #[allow(dead_code, non_camel_case_types)]
                enum Idents { $( $idents, )* __CountIdentsLast }
                const COUNT: usize = Idents::__CountIdentsLast as usize;
                COUNT
            }
        };
    }

    // Here we go! Let's generate all the C struct, `WasmTypeList`
    // implementations and `HostFunction` implementations.
    impl_host_function!([C] S0,);
    impl_host_function!([transparent] S1, A1);
    impl_host_function!([C] S2, A1, A2);
    impl_host_function!([C] S3, A1, A2, A3);
    impl_host_function!([C] S4, A1, A2, A3, A4);
    impl_host_function!([C] S5, A1, A2, A3, A4, A5);
    impl_host_function!([C] S6, A1, A2, A3, A4, A5, A6);
    impl_host_function!([C] S7, A1, A2, A3, A4, A5, A6, A7);
    impl_host_function!([C] S8, A1, A2, A3, A4, A5, A6, A7, A8);
    impl_host_function!([C] S9, A1, A2, A3, A4, A5, A6, A7, A8, A9);
    impl_host_function!([C] S10, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
    impl_host_function!([C] S11, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
    impl_host_function!([C] S12, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
    impl_host_function!([C] S13, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
    impl_host_function!([C] S14, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
    impl_host_function!([C] S15, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
    impl_host_function!([C] S16, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
    impl_host_function!([C] S17, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17);
    impl_host_function!([C] S18, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18);
    impl_host_function!([C] S19, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19);
    impl_host_function!([C] S20, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20);
    impl_host_function!([C] S21, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21);
    impl_host_function!([C] S22, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22);
    impl_host_function!([C] S23, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23);
    impl_host_function!([C] S24, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24);
    impl_host_function!([C] S25, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25);
    impl_host_function!([C] S26, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26);

    // Implement `WasmTypeList` on `Infallible`, which means that
    // `Infallible` can be used as a returned type of a host function
    // to express that it doesn't return, or to express that it cannot
    // fail (with `Result<_, Infallible>`).
    impl WasmTypeList for Infallible {
        type CStruct = Self;
        type Array = [f64; 0];

        fn size() -> u32 {
            0
        }

        unsafe fn from_array(_: &mut impl AsStoreMut, _: Self::Array) -> Self {
            unreachable!()
        }

        unsafe fn from_slice(
            _: &mut impl AsStoreMut,
            _: &[f64],
        ) -> Result<Self, TryFromSliceError> {
            unreachable!()
        }

        unsafe fn into_array(self, _: &mut impl AsStoreMut) -> Self::Array {
            []
        }

        fn empty_array() -> Self::Array {
            []
        }

        unsafe fn from_c_struct(_: &mut impl AsStoreMut, self_: Self::CStruct) -> Self {
            self_
        }

        unsafe fn into_c_struct(self, _: &mut impl AsStoreMut) -> Self::CStruct {
            self
        }

        unsafe fn write_c_struct_to_ptr(_: Self::CStruct, _: *mut f64) {}

        fn wasm_types() -> &'static [Type] {
            &[]
        }
    }

    /*
        #[cfg(test)]
        mod test_wasm_type_list {
            use super::*;
            use wasmer_types::Type;

            fn test_from_array() {
                assert_eq!(<()>::from_array([]), ());
                assert_eq!(<i32>::from_array([1]), (1i32));
                assert_eq!(<(i32, i64)>::from_array([1, 2]), (1i32, 2i64));
                assert_eq!(
                    <(i32, i64, f32, f64)>::from_array([
                        1,
                        2,
                        (3.1f32).to_bits().into(),
                        (4.2f64).to_bits().into()
                    ]),
                    (1, 2, 3.1f32, 4.2f64)
                );
            }

            fn test_into_array() {
                assert_eq!(().into_array(), [0; 0]);
                assert_eq!((1).into_array(), [1]);
                assert_eq!((1i32, 2i64).into_array(), [1, 2]);
                assert_eq!(
                    (1i32, 2i32, 3.1f32, 4.2f64).into_array(),
                    [1, 2, (3.1f32).to_bits().into(), (4.2f64).to_bits().into()]
                );
            }

            fn test_empty_array() {
                assert_eq!(<()>::empty_array().len(), 0);
                assert_eq!(<i32>::empty_array().len(), 1);
                assert_eq!(<(i32, i64)>::empty_array().len(), 2);
            }

            fn test_from_c_struct() {
                assert_eq!(<()>::from_c_struct(S0()), ());
                assert_eq!(<i32>::from_c_struct(S1(1)), (1i32));
                assert_eq!(<(i32, i64)>::from_c_struct(S2(1, 2)), (1i32, 2i64));
                assert_eq!(
                    <(i32, i64, f32, f64)>::from_c_struct(S4(1, 2, 3.1, 4.2)),
                    (1i32, 2i64, 3.1f32, 4.2f64)
                );
            }

            fn test_wasm_types_for_uni_values() {
                assert_eq!(<i32>::wasm_types(), [Type::I32]);
                assert_eq!(<i64>::wasm_types(), [Type::I64]);
                assert_eq!(<f32>::wasm_types(), [Type::F32]);
                assert_eq!(<f64>::wasm_types(), [Type::F64]);
            }

            fn test_wasm_types_for_multi_values() {
                assert_eq!(<(i32, i32)>::wasm_types(), [Type::I32, Type::I32]);
                assert_eq!(<(i64, i64)>::wasm_types(), [Type::I64, Type::I64]);
                assert_eq!(<(f32, f32)>::wasm_types(), [Type::F32, Type::F32]);
                assert_eq!(<(f64, f64)>::wasm_types(), [Type::F64, Type::F64]);

                assert_eq!(
                    <(i32, i64, f32, f64)>::wasm_types(),
                    [Type::I32, Type::I64, Type::F32, Type::F64]
                );
            }
        }

        #[allow(non_snake_case)]
        #[cfg(test)]
        mod test_function {
            use super::*;
            use wasmer_types::Type;

            fn func() {}
            fn func__i32() -> i32 {
                0
            }
            fn func_i32(_a: i32) {}
            fn func_i32__i32(a: i32) -> i32 {
                a * 2
            }
            fn func_i32_i32__i32(a: i32, b: i32) -> i32 {
                a + b
            }
            fn func_i32_i32__i32_i32(a: i32, b: i32) -> (i32, i32) {
                (a, b)
            }
            fn func_f32_i32__i32_f32(a: f32, b: i32) -> (i32, f32) {
                (b, a)
            }

            fn test_function_types() {
                assert_eq!(Function::new(func).ty(), FunctionType::new(vec![], vec![]));
                assert_eq!(
                    Function::new(func__i32).ty(),
                    FunctionType::new(vec![], vec![Type::I32])
                );
                assert_eq!(
                    Function::new(func_i32).ty(),
                    FunctionType::new(vec![Type::I32], vec![])
                );
                assert_eq!(
                    Function::new(func_i32__i32).ty(),
                    FunctionType::new(vec![Type::I32], vec![Type::I32])
                );
                assert_eq!(
                    Function::new(func_i32_i32__i32).ty(),
                    FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32])
                );
                assert_eq!(
                    Function::new(func_i32_i32__i32_i32).ty(),
                    FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32, Type::I32])
                );
                assert_eq!(
                    Function::new(func_f32_i32__i32_f32).ty(),
                    FunctionType::new(vec![Type::F32, Type::I32], vec![Type::I32, Type::F32])
                );
            }

            fn test_function_pointer() {
                let f = Function::new(func_i32__i32);
                let function = unsafe { std::mem::transmute::<_, fn(usize, i32) -> i32>(f.address) };
                assert_eq!(function(0, 3), 6);
            }
        }
    */
}
