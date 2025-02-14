use wasmer_types::{FunctionType, RawValue};

use crate::{
    error::RuntimeError,
    macros::backend::{gen_rt_ty, match_rt},
    vm::{VMExtern, VMExternFunction, VMFuncRef},
    AsStoreMut, AsStoreRef, ExportError, Exportable, Extern, FunctionEnv, FunctionEnvMut,
    HostFunction, StoreMut, StoreRef, TypedFunction, Value, WasmTypeList, WithEnv, WithoutEnv,
};

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
gen_rt_ty!(Function
    @cfg feature = "artifact-size" => derive(loupe::MemoryUsage)
    @derives Debug, Clone, PartialEq, Eq
);

impl BackendFunction {
    /// Creates a new host `Function` (dynamic) with the provided signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Self::new_typed`] for less runtime overhead.
    #[inline]
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

    /// Creates a new host `Function` (dynamic) with the provided signature.
    ///
    /// If you know the signature of the host function at compile time,
    /// consider using [`Self::new_typed_with_env`] for less runtime overhead.
    ///
    /// Takes a [`FunctionEnv`] that is passed into func. If that is not required,
    /// [`Self::new`] might be an option as well.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionType, Type, Store, Value};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// let signature = FunctionType::new(vec![Type::I32, Type::I32], vec![Type::I32]);
    ///
    /// let f = Function::new_with_env(&mut store, &env, &signature, |_env, args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    ///
    /// With constant signature:
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionType, Type, Store, Value};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// const I32_I32_TO_I32: ([Type; 2], [Type; 1]) = ([Type::I32, Type::I32], [Type::I32]);
    ///
    /// let f = Function::new_with_env(&mut store, &env, I32_I32_TO_I32, |_env, args| {
    ///     let sum = args[0].unwrap_i32() + args[1].unwrap_i32();
    ///     Ok(vec![Value::I32(sum)])
    /// });
    /// ```
    #[inline]
    pub fn new_with_env<FT, F, T: Send + 'static>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(FunctionEnvMut<T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::entities::function::Function::new_with_env(
                    store, env, ty, func,
                ),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::entities::function::Function::new_with_env(
                    store, env, ty, func,
                ),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::entities::function::Function::new_with_env(
                    store, env, ty, func,
                ),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(
                crate::backend::v8::entities::function::Function::new_with_env(
                    store, env, ty, func,
                ),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(
                crate::backend::js::entities::function::Function::new_with_env(
                    store, env, ty, func,
                ),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::entities::function::Function::new_with_env(
                    store, env, ty, func,
                ),
            ),
        }
    }

    /// Creates a new host `Function` from a native function.
    #[inline]
    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => {
                Self::Sys(crate::backend::sys::entities::function::Function::new_typed(store, func))
            }
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::entities::function::Function::new_typed(store, func),
            ),

            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::entities::function::Function::new_typed(store, func),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(
                crate::backend::v8::entities::function::Function::new_typed(store, func),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(
                crate::backend::js::entities::function::Function::new_typed(store, func),
            ),

            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => {
                Self::Jsc(crate::backend::jsc::entities::function::Function::new_typed(store, func))
            }
        }
    }

    /// Creates a new host `Function` with an environment from a typed function.
    ///
    /// The function signature is automatically retrieved using the
    /// Rust typing system.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Store, Function, FunctionEnv, FunctionEnvMut};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    /// ```
    #[inline]
    pub fn new_typed_with_env<T: Send + 'static, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        F: HostFunction<T, Args, Rets, WithEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(s) => Self::Sys(
                crate::backend::sys::entities::function::Function::new_typed_with_env(
                    store, env, func,
                ),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(s) => Self::Wamr(
                crate::backend::wamr::entities::function::Function::new_typed_with_env(
                    store, env, func,
                ),
            ),

            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(s) => Self::Wasmi(
                crate::backend::wasmi::entities::function::Function::new_typed_with_env(
                    store, env, func,
                ),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(s) => Self::V8(
                crate::backend::v8::entities::function::Function::new_typed_with_env(
                    store, env, func,
                ),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(s) => Self::Js(
                crate::backend::js::entities::function::Function::new_typed_with_env(
                    store, env, func,
                ),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(s) => Self::Jsc(
                crate::backend::jsc::entities::function::Function::new_typed_with_env(
                    store, env, func,
                ),
            ),
        }
    }

    /// Returns the [`FunctionType`] of the `Function`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionEnvMut, Store, Type};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    ///
    /// assert_eq!(f.ty(&mut store).params(), vec![Type::I32, Type::I32]);
    /// assert_eq!(f.ty(&mut store).results(), vec![Type::I32]);
    /// ```
    #[inline]
    pub fn ty(&self, store: &impl AsStoreRef) -> FunctionType {
        match_rt!(on self => f {
            f.ty(store)
        })
    }

    /// Returns the number of parameters that this function takes.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Function, FunctionEnv, FunctionEnvMut, Store, Type};
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    ///
    /// assert_eq!(f.param_arity(&mut store), 2);
    /// ```
    #[inline]
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
    /// # let env = FunctionEnv::new(&mut store, ());
    /// #
    /// fn sum(_env: FunctionEnvMut<()>, a: i32, b: i32) -> i32 {
    ///     a + b
    /// }
    ///
    /// let f = Function::new_typed_with_env(&mut store, &env, sum);
    ///
    /// assert_eq!(f.result_arity(&mut store), 1);
    /// ```
    #[inline]
    pub fn result_arity(&self, store: &impl AsStoreRef) -> usize {
        self.ty(store).params().len()
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
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
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
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// assert_eq!(sum.call(&mut store, &[Value::I32(1), Value::I32(2)]).unwrap().to_vec(), vec![Value::I32(3)]);
    /// ```
    #[inline]
    pub fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        match_rt!(on self => f {
            f.call(store, params)
        })
    }

    #[doc(hidden)]
    #[allow(missing_docs)]
    #[inline]
    pub fn call_raw(
        &self,
        store: &mut impl AsStoreMut,
        params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        match_rt!(on self => f {
            f.call_raw(store, params)
        })
    }

    #[inline]
    pub(crate) fn vm_funcref(&self, store: &impl AsStoreRef) -> VMFuncRef {
        match self {
            #[cfg(feature = "sys")]
            Self::Sys(f) => VMFuncRef::Sys(f.vm_funcref(store)),
            #[cfg(feature = "wamr")]
            Self::Wamr(f) => VMFuncRef::Wamr(f.vm_funcref(store)),
            #[cfg(feature = "wasmi")]
            Self::Wasmi(f) => VMFuncRef::Wasmi(f.vm_funcref(store)),
            #[cfg(feature = "v8")]
            Self::V8(f) => VMFuncRef::V8(f.vm_funcref(store)),
            #[cfg(feature = "js")]
            Self::Js(f) => VMFuncRef::Js(f.vm_funcref(store)),
            #[cfg(feature = "jsc")]
            Self::Jsc(f) => VMFuncRef::Jsc(f.vm_funcref(store)),
        }
    }

    #[inline]
    pub(crate) unsafe fn from_vm_funcref(store: &mut impl AsStoreMut, funcref: VMFuncRef) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(s) => Self::Sys(
                crate::backend::sys::entities::function::Function::from_vm_funcref(
                    store,
                    funcref.into_sys(),
                ),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(s) => Self::Wamr(
                crate::backend::wamr::entities::function::Function::from_vm_funcref(
                    store,
                    funcref.into_wamr(),
                ),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(s) => Self::Wasmi(
                crate::backend::wasmi::entities::function::Function::from_vm_funcref(
                    store,
                    funcref.into_wasmi(),
                ),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(s) => Self::V8(
                crate::backend::v8::entities::function::Function::from_vm_funcref(
                    store,
                    funcref.into_v8(),
                ),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(s) => Self::Js(
                crate::backend::js::entities::function::Function::from_vm_funcref(
                    store,
                    funcref.into_js(),
                ),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(s) => Self::Jsc(
                crate::backend::jsc::entities::function::Function::from_vm_funcref(
                    store,
                    funcref.into_jsc(),
                ),
            ),
        }
    }

    /// Transform this WebAssembly function into a typed function.
    /// See [`TypedFunction`] to learn more.
    ///
    /// # Examples
    ///
    /// ```
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, TypedFunction, Value};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
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
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    /// let sum_typed: TypedFunction<(i32, i32), i32> = sum.typed(&mut store).unwrap();
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
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, TypedFunction, Value};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
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
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_typed : TypedFunction<(i64, i64), i32> = sum.typed(&mut store).unwrap();
    /// ```
    ///
    /// If the `Rets` generic parameter does not match the exported function
    /// an error will be raised:
    ///
    /// ```should_panic
    /// # use wasmer::{imports, wat2wasm, Function, Instance, Module, Store, Type, TypedFunction, Value};
    /// # use wasmer::FunctionEnv;
    /// # let mut store = Store::default();
    /// # let env = FunctionEnv::new(&mut store, ());
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
    /// # let instance = Instance::new(&mut store, &module, &import_object).unwrap();
    /// #
    /// let sum = instance.exports.get_function("sum").unwrap();
    ///
    /// // This results in an error: `RuntimeError`
    /// let sum_typed: TypedFunction<(i32, i32), i64> = sum.typed(&mut store).unwrap();
    /// ```
    #[inline]
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
                    "given types (`{given:?}`) for the function arguments don't match the actual types (`{expected:?}`)",
                )));
            }
        }

        {
            let expected = ty.results();
            let given = Rets::wasm_types();

            if expected != given {
                // todo: error result types don't match
                return Err(RuntimeError::new(format!(
                    "given types (`{given:?}`) for the function results don't match the actual types (`{expected:?}`)",
                )));
            }
        }

        Ok(TypedFunction::new(store, super::Function(self.clone())))
    }

    pub(crate) fn from_vm_extern(store: &mut impl AsStoreMut, vm_extern: VMExternFunction) -> Self {
        match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => Self::Sys(
                crate::backend::sys::entities::function::Function::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => Self::Wamr(
                crate::backend::wamr::entities::function::Function::from_vm_extern(
                    store, vm_extern,
                ),
            ),
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => Self::Wasmi(
                crate::backend::wasmi::entities::function::Function::from_vm_extern(
                    store, vm_extern,
                ),
            ),
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => Self::V8(
                crate::backend::v8::entities::function::Function::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => Self::Js(
                crate::backend::js::entities::function::Function::from_vm_extern(store, vm_extern),
            ),
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => Self::Jsc(
                crate::backend::jsc::entities::function::Function::from_vm_extern(store, vm_extern),
            ),
        }
    }

    /// Checks whether this `Function` can be used with the given store.
    #[inline]
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        match_rt!(on self => f {
            f.is_from_store(store)
        })
    }

    #[inline]
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        match_rt!(on self => f {
            f.to_vm_extern()
        })
    }
}

impl<'a> Exportable<'a> for BackendFunction {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Function(func) => Ok(&func.0),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
