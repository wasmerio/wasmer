pub(crate) mod env;
pub(crate) mod numeric;
pub(crate) mod typed;
use std::{cell::RefCell, collections::HashMap, marker::PhantomData};
#[cfg(feature = "experimental-async")]
use std::{future::Future, pin::Pin, rc::Rc, sync::Arc, task::Poll};

pub(crate) use typed::*;

use js_sys::{Array, Function as JsFunction};
#[cfg(feature = "experimental-async")]
use js_sys::Reflect;
use wasm_bindgen::{JsCast, prelude::*};
use wasmer_types::{FunctionType, RawValue, Type};

use crate::{
    AsStoreMut, AsStoreRef, BackendFunction, BackendFunctionEnv, BackendFunctionEnvMut,
    CallWrapper, FromToNativeWasmType, FunctionEnv, FunctionEnvMut, HostFunction,
    HostFunctionKind, IntoResult, NativeWasmType, NativeWasmTypeInto, RuntimeError, StoreMut,
    Value, WasmTypeList, WithEnv, WithoutEnv,
    entities::function::call_wrapper::call_wrapped,
    js::{
        store::StoreSlot,
        utils::convert::{AsJs as _, js_value_to_wasmer, wasmer_value_to_js},
        vm::{VMFuncRef, VMFunctionCallback, function::VMFunction},
    },
    vm::{VMExtern, VMExternFunction},
};
#[cfg(feature = "experimental-async")]
use crate::{
    AsStoreAsync, AsyncFunctionEnvMut, BackendAsyncFunctionEnvMut, StoreAsync, StoreContext,
    StoreInner,
    entities::function::async_host::{AsyncFunctionEnv, AsyncHostFunction},
    js::{
        function::env::{
            AsyncFunctionEnvMut as JsAsyncFunctionEnvMut, AsyncFunctionEnvMutStore,
            FunctionEnv as JsFunctionEnv,
        },
        jspi,
    },
};

use std::panic::{self, AssertUnwindSafe};

#[derive(Debug)]
struct HostFunctionPanic(String);

impl std::fmt::Display for HostFunctionPanic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HostFunctionPanic {}

fn raise_host_function_panic(payload: Box<dyn std::any::Any + Send>) -> ! {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "host function panicked with a non-string payload".to_owned()
    };
    crate::backend::js::error::raise(Box::new(HostFunctionPanic(message)))
}

/// What a dynamic host function returned for a result of type `ty`, as the
/// engine takes it: an `i64` is a BigInt there, and a Number for that result
/// fails the call. Any other value goes as it always did, for the engine to
/// convert to the type of the result.
#[inline]
fn host_result_to_js(ty: &Type, value: &Value) -> JsValue {
    match (ty, value) {
        (Type::I64, Value::I64(value)) => JsValue::from(*value),
        _ => wasmer_value_to_js(value),
    }
}

/// The results of a dynamic host function with `types` of results, as the
/// engine takes them. Values beyond those types go too, as they always did.
#[inline]
fn host_results_to_js(types: &[Type], values: &[Value]) -> Array {
    Array::from_iter(values.iter().enumerate().map(|(index, value)| {
        match types.get(index) {
            Some(ty) => host_result_to_js(ty, value),
            None => wasmer_value_to_js(value),
        }
    }))
}

#[derive(Clone, PartialEq, Eq)]
pub struct Function {
    pub(crate) handle: VMFunction,
}

// Function can't be Send in js because it doesn't support `structuredClone`
// https://developer.mozilla.org/en-US/docs/Web/API/structuredClone
// unsafe impl Send for Function {}

impl From<VMFunction> for Function {
    fn from(handle: VMFunction) -> Self {
        Self { handle }
    }
}

impl Function {
    /// To `VMExtern`.
    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Js(crate::js::vm::external::VMExtern::Function(
            self.handle.clone(),
        ))
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_async<FT, F, Fut>(
        store: &mut impl AsStoreMut,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(&[Value]) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
    {
        let env = FunctionEnv::new(store, ());
        Self::new_with_env_async(store, &env, ty, move |_env, values| func(values))
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_with_env_async<FT, F, Fut, T>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        ty: FT,
        func: F,
    ) -> Self
    where
        FT: Into<FunctionType>,
        F: Fn(AsyncFunctionEnvMut<T>, &[Value]) -> Fut + 'static,
        Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
        T: 'static,
    {
        assert!(
            jspi::is_supported(),
            "the JavaScript host does not support WebAssembly JSPI"
        );

        let func_ty: FunctionType = ty.into();
        let host = Rc::new(AsyncHost {
            func,
            env: env.as_js().clone(),
            ty: func_ty.clone(),
            store_id: store.objects_mut().id(),
            slot: StoreSlot::of(store),
        });
        let adapter = move |eager: bool| -> JsFunction {
            let start = Rc::clone(&host).start(eager);
            match jspi::adapter(&host.ty, &start, eager, numeric::is_numeric(&host.ty)) {
                Ok(function) => function,
                // A signature that cannot be an async host function.
                Err(error) if error.is_string() => panic!("{}", error.as_string().unwrap()),
                Err(error) => wasm_bindgen::throw_val(error),
            }
        };
        let mut vm_function = VMFunction::new(adapter(false), func_ty).of_store(store);
        vm_function.eager = Some(Rc::new(move || adapter(true)));
        Self::from_vm_extern(
            &mut store.as_store_mut(),
            VMExternFunction::Js(vm_function),
        )
    }

    /// The eager form of an async host function; any other function as it is.
    #[cfg(feature = "experimental-async")]
    pub(crate) fn eager(self) -> Self {
        match &self.handle.eager {
            Some(eager) => Self {
                handle: VMFunction {
                    store: self.handle.store,
                    ..VMFunction::new(eager(), self.handle.ty.clone())
                },
            },
            None => self,
        }
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_typed_async<F, Args, Rets>(
        store: &mut impl AsStoreMut,
        func: F,
    ) -> Self
    where
        Args: WasmTypeList + 'static,
        Rets: WasmTypeList + 'static,
        F: AsyncHostFunction<(), Args, Rets, WithoutEnv> + 'static,
    {
        let env = FunctionEnv::new(store, ());
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        let args_sig = Arc::new(signature.clone());
        let results_sig = Arc::new(signature.clone());
        let func = Arc::new(func);
        Self::new_with_env_async(
            store,
            &env,
            signature,
            move |mut env_mut, values| -> Pin<
                Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>,
            > {
                let js_env = match env_mut.0 {
                    BackendAsyncFunctionEnvMut::Js(ref mut js_env) => js_env,
                    _ => panic!("Not a js backend"),
                };
                let mut store_wrapper = unsafe { StoreContext::get_current(js_env.store_id()) };
                let mut store_mut = store_wrapper.as_mut();
                let args = match typed_args_from_values::<Args>(
                    &mut store_mut,
                    args_sig.as_ref(),
                    values,
                ) {
                    Ok(args) => args,
                    Err(error) => return Box::pin(async { Err(error) }),
                };
                drop(store_wrapper);
                let js_env = js_env.clone();
                let func = Arc::clone(&func);
                let results_sig = Arc::clone(&results_sig);
                let future = func
                    .as_ref()
                    .call_async(AsyncFunctionEnv::new(), args);
                Box::pin(async move {
                    let typed_result = future.await?;
                    let to_values = |store: &mut StoreMut<'_>| {
                        typed_results_to_values::<Rets>(store, results_sig.as_ref(), typed_result)
                    };
                    js_env.with_store_mut(to_values).await
                })
            },
        )
    }

    #[cfg(feature = "experimental-async")]
    pub(crate) fn new_typed_with_env_async<T, F, Args, Rets>(
        store: &mut impl AsStoreMut,
        env: &FunctionEnv<T>,
        func: F,
    ) -> Self
    where
        T: 'static,
        F: AsyncHostFunction<T, Args, Rets, WithEnv> + 'static,
        Args: WasmTypeList + 'static,
        Rets: WasmTypeList + 'static,
    {
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        let args_sig = Arc::new(signature.clone());
        let results_sig = Arc::new(signature.clone());
        let func = Arc::new(func);
        Self::new_with_env_async(
            store,
            env,
            signature,
            move |mut env_mut, values| -> Pin<
                Box<dyn Future<Output = Result<Vec<Value>, RuntimeError>>>,
            > {
                let js_env = match env_mut.0 {
                    BackendAsyncFunctionEnvMut::Js(ref mut js_env) => js_env,
                    _ => panic!("Not a js backend"),
                };
                let mut store_wrapper = unsafe { StoreContext::get_current(js_env.store_id()) };
                let mut store_mut = store_wrapper.as_mut();
                let args = match typed_args_from_values::<Args>(
                    &mut store_mut,
                    args_sig.as_ref(),
                    values,
                ) {
                    Ok(args) => args,
                    Err(error) => return Box::pin(async { Err(error) }),
                };
                drop(store_wrapper);
                let js_env = js_env.clone();
                let func = Arc::clone(&func);
                let results_sig = Arc::clone(&results_sig);
                let future = func
                    .as_ref()
                    .call_async(AsyncFunctionEnv::with_env(env_mut), args);
                Box::pin(async move {
                    let typed_result = future.await?;
                    let to_values = |store: &mut StoreMut<'_>| {
                        typed_results_to_values::<Rets>(store, results_sig.as_ref(), typed_result)
                    };
                    js_env.with_store_mut(to_values).await
                })
            },
        )
    }

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
        let slot = StoreSlot::of(&store);
        let raw_env = env.clone();
        if numeric::is_numeric(&function_type) {
            // Nothing but numbers: the guest passes them as they are, with
            // no array of arguments in between.
            let arity = function_type.params().len();
            let function = numeric::host(arity, move |numbers| {
                // SAFETY: a guest calls this host function of the store.
                let mut store = unsafe { slot.store()? };
                let arguments = numeric::values(function_type.params(), numbers);
                let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
                let results = func(env, &arguments)?;
                Ok(match function_type.results() {
                    [] => 0.0,
                    _ => numeric::result(&results),
                })
            });
            let vm_function = VMFunction::new(function, func_ty).of_store(&store);
            return Self::from_vm_extern(&mut store, VMExternFunction::Js(vm_function));
        }
        let wrapped_func: JsValue = match function_type.results().len() {
            0 => Closure::wrap(Box::new(move |args: &Array| {
                // SAFETY: a guest calls this host function of the store.
                let mut store = unsafe { slot.store()? };
                let wasm_arguments = function_type
                    .params()
                    .iter()
                    .enumerate()
                    .map(|(i, param)| {
                        js_value_to_wasmer(&mut store, param, &args.get(i as u32))
                    })
                    .collect::<Vec<_>>();
                let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
                let _results = func(env, &wasm_arguments)?;
                Ok(())
            })
                as Box<dyn Fn(&Array) -> Result<(), JsValue>>)
            .into_js_value(),
            1 => Closure::wrap(Box::new(move |args: &Array| {
                // SAFETY: a guest calls this host function of the store.
                let mut store = unsafe { slot.store()? };
                let wasm_arguments = function_type
                    .params()
                    .iter()
                    .enumerate()
                    .map(|(i, param)| {
                        js_value_to_wasmer(&mut store, param, &args.get(i as u32))
                    })
                    .collect::<Vec<_>>();
                let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
                let results = func(env, &wasm_arguments)?;
                Ok(host_result_to_js(&function_type.results()[0], &results[0]))
            })
                as Box<dyn Fn(&Array) -> Result<JsValue, JsValue>>)
            .into_js_value(),
            _n => Closure::wrap(Box::new(move |args: &Array| {
                // SAFETY: a guest calls this host function of the store.
                let mut store = unsafe { slot.store()? };
                let wasm_arguments = function_type
                    .params()
                    .iter()
                    .enumerate()
                    .map(|(i, param)| {
                        js_value_to_wasmer(&mut store, param, &args.get(i as u32))
                    })
                    .collect::<Vec<_>>();
                let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
                let results = func(env, &wasm_arguments)?;
                Ok(host_results_to_js(function_type.results(), &results))
            })
                as Box<dyn Fn(&Array) -> Result<Array, JsValue>>)
            .into_js_value(),
        };

        let binded_func = with_arguments_array(&wrapped_func);
        let vm_function = VMFunction::new(binded_func, func_ty).of_store(&store);
        Self::from_vm_extern(&mut store, VMExternFunction::Js(vm_function))
    }

    /// Creates a new host `Function` from a native function.
    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let store = store.as_store_mut();
        if std::mem::size_of::<F>() != 0 {
            Self::closures_unsupported_panic();
        }
        let function = WasmFunction::<Args, Rets>::new(func);
        let address = function.address() as usize as u32;

        let ft = wasm_bindgen::function_table();
        let as_table = ft.unchecked_ref::<js_sys::WebAssembly::Table>();
        let func = as_table.get(address).unwrap();

        let ty = function.ty();
        // The function is given the address of the slot of its store, and
        // keeps the slot alive.
        let slot = StoreSlot::of(&store);
        let leading = [JsValue::from_f64(slot.address())];
        let binded_func = with_leading_arguments(func.unchecked_ref(), &leading, ty.params().len());
        slot.keep_alive_by(&binded_func);
        let vm_function = VMFunction::new(binded_func, ty).of_store(&store);
        Self {
            handle: vm_function,
        }
    }

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
        let store = store.as_store_mut();
        if std::mem::size_of::<F>() != 0 {
            Self::closures_unsupported_panic();
        }
        let function = WasmFunction::<Args, Rets>::new(func);
        let address = function.address() as usize as u32;

        let ft = wasm_bindgen::function_table();
        let as_table = ft.unchecked_ref::<js_sys::WebAssembly::Table>();
        let func = as_table.get(address).unwrap();

        let ty = function.ty();
        let slot = StoreSlot::of(&store);
        let leading = [
            JsValue::from_f64(slot.address()),
            JsValue::from_f64(env.as_js().handle.internal_handle().index() as f64),
        ];
        let binded_func = with_leading_arguments(func.unchecked_ref(), &leading, ty.params().len());
        slot.keep_alive_by(&binded_func);
        let vm_function = VMFunction::new(binded_func, ty).of_store(&store);
        Self {
            handle: vm_function,
        }
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> FunctionType {
        self.handle.ty.clone()
    }

    pub fn call_raw(
        &self,
        _store: &mut impl AsStoreMut,
        _params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        // There is no optimal call_raw in JS, so we just
        // simply rely the call
        // self.call(store, params)
        unimplemented!();
    }

    pub fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        self.call_wrapped(store, None, params)
    }

    pub(crate) fn call_wrapped(
        &self,
        store: &mut impl AsStoreMut,
        wrapper: Option<&mut dyn CallWrapper>,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        let ty = &self.handle.ty;
        if self.handle.numeric {
            // Nothing but numbers: they are passed as they are, i32s as i32s
            // (see `numeric::Arguments`), other numbers as f64s.
            let function = &self.handle.function;
            let has_result = !ty.results().is_empty();
            let mut result = 0.0;
            let results = |result: f64| -> Box<[Value]> {
                let results = ty.results().iter();
                results.map(|ty| numeric::value(ty, result)).collect()
            };
            if let Some(integers) = numeric::integers(ty, params) {
                call_wrapped(store, wrapper, |_| {
                    result = numeric::call_integers(function, &integers, has_result)?;
                    Ok(())
                })?;
                return Ok(results(result));
            }
            if let Some(numbers) = numeric::numbers(ty, params) {
                call_wrapped(store, wrapper, |_| {
                    result = numeric::call(function, &numbers, has_result)?;
                    Ok(())
                })?;
                return Ok(results(result));
            }
        }

        // Annotation is here to prevent spurious IDE warnings.
        let arr = js_sys::Array::new_with_length(params.len() as u32);

        // let raw_env = env.as_raw() as *mut u8;
        // let mut env = unsafe { FunctionEnvMut::from_raw(raw_env as *mut StoreInner<()>) };

        for (i, param) in params.iter().enumerate() {
            let js_value = param.as_jsvalue(&store.as_store_ref());
            arr.set(i as u32, js_value);
        }

        let mut result = JsValue::UNDEFINED;
        call_wrapped(store, wrapper, |_| {
            result = js_sys::Reflect::apply(&self.handle.function, &JsValue::NULL, &arr)?;
            Ok(())
        })?;

        let result_types = self.handle.ty.results();
        match result_types.len() {
            0 => Ok(Box::new([])),
            1 => {
                let value = js_value_to_wasmer(store, &result_types[0], &result);
                Ok(vec![value].into_boxed_slice())
            }
            _n => {
                let result_array: Array = result.into();
                Ok(result_array
                    .iter()
                    .enumerate()
                    .map(|(i, js_val)| {
                        js_value_to_wasmer(store, &result_types[i], &js_val)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice())
            }
        }
    }

    #[cfg(feature = "experimental-async")]
    #[allow(clippy::type_complexity)]
    pub(crate) fn call_async(
        &self,
        store: &impl AsStoreAsync,
        params: Vec<Value>,
    ) -> Pin<Box<dyn Future<Output = Result<Box<[Value]>, RuntimeError>> + 'static>> {
        let function = self.clone();
        let store = store.store();
        Box::pin(async move {
            let write_lock = store.write_lock().await;
            let arguments = Array::new_with_length(params.len() as u32);
            for (index, param) in params.iter().enumerate() {
                arguments.set(index as u32, param.as_jsvalue(&write_lock));
            }

            let promising = match function.handle.promising.get() {
                Some(promising) => promising,
                None => {
                    let promising = jspi::promising(&function.handle.function)
                        .map_err(RuntimeError::from)?;
                    function.handle.promising.get_or_init(|| promising)
                }
            };
            let caller = std::future::poll_fn(|cx| Poll::Ready(cx.waker().clone())).await;
            // The guest runs with the write lock, in this span and in every
            // later one. Dropping this future while it waits cancels the call.
            let result = jspi::Call::enter(&store, write_lock, caller, || {
                Reflect::apply(promising, &JsValue::NULL, &arguments)
            })
            .await?;
            let mut write_lock = store.write_lock().await;
            let function_type = &function.handle.ty;
            match function_type.results().len() {
                0 => Ok(Box::<[Value]>::default()),
                1 => Ok(vec![js_value_to_wasmer(
                    &mut write_lock,
                    &function_type.results()[0],
                    &result,
                )]
                .into_boxed_slice()),
                _ => {
                    let result: Array = result.into();
                    Ok(function_type
                        .results()
                        .iter()
                        .enumerate()
                        .map(|(index, ty)| {
                            js_value_to_wasmer(
                                &mut write_lock,
                                ty,
                                &result.get(index as u32),
                            )
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice())
                }
            }
        })
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMExternFunction) -> Self {
        Self {
            handle: internal.unwrap_js(),
        }
    }

    pub(crate) fn vm_funcref(&self, _store: &impl AsStoreRef) -> VMFuncRef {
        unimplemented!();
    }

    pub(crate) unsafe fn from_vm_funcref(
        _store: &mut impl AsStoreMut,
        _funcref: VMFuncRef,
    ) -> Self {
        unimplemented!();
    }

    #[track_caller]
    fn closures_unsupported_panic() -> ! {
        unimplemented!(
            "Closures (functions with captured environments) are currently unsupported with native functions. See: https://github.com/wasmerio/wasmer/issues/1840"
        )
    }

    /// Checks whether this `Function` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.is_from_store(store)
    }
}

#[wasm_bindgen]
extern "C" {
    /// `new Function(parameters, body)`, which a host may forbid.
    #[wasm_bindgen(js_name = Function)]
    type EvaluatedFunction;
    #[wasm_bindgen(constructor, js_class = "Function", catch)]
    fn new(parameters: &str, body: &str) -> Result<EvaluatedFunction, JsValue>;
}

thread_local! {
    /// `(f, ...arguments) => f(arguments)`, for `with_arguments_array`, or
    /// `None` where the host does not let functions be made from text.
    static ARGUMENTS_ARRAY: Option<JsFunction> = EvaluatedFunction::new(
        "f",
        "return f(Array.prototype.slice.call(arguments, 1))",
    )
    .ok()
    .map(JsCast::unchecked_into);

    /// What makes the functions of `with_leading_arguments`, by number of
    /// leading arguments and of parameters. `None` where the host does not
    /// let functions be made from text.
    static LEADING_ARGUMENTS: RefCell<HashMap<(usize, usize), Option<JsFunction>>> =
        RefCell::new(HashMap::new());
}

/// A function that calls `function` with one argument, the array of its own
/// arguments. Where the host does not let functions be made from text, it is
/// a proxy of `function` whose `apply` trap is given that array.
fn with_arguments_array(function: &JsValue) -> JsFunction {
    if let Some(spread) = ARGUMENTS_ARRAY.with(Clone::clone) {
        return spread.bind1(&JsValue::UNDEFINED, function).unchecked_into();
    }
    let target = function.clone();
    let apply = Closure::<dyn Fn(JsValue, JsValue, Array) -> Result<JsValue, JsValue>>::new(
        move |_: JsValue, _: JsValue, arguments: Array| {
            target
                .unchecked_ref::<JsFunction>()
                .call1(&JsValue::UNDEFINED, &arguments)
        },
    );
    let handler = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&handler, &"apply".into(), &apply.into_js_value());
    js_sys::Proxy::new(function, &handler).unchecked_into()
}

/// A function of `arity` parameters that calls `function` with `leading` and
/// then its own arguments: `function.bind(undefined, ...leading)`, as a plain
/// arrow function of that many parameters where the host allows making one.
/// Engines call such an arrow faster than a bound function when the caller is
/// WebAssembly and `function` is too: JavaScriptCore several times faster.
fn with_leading_arguments(function: &JsFunction, leading: &[JsValue], arity: usize) -> JsFunction {
    arrow_with_leading_arguments(function, leading, arity)
        .unwrap_or_else(|| bound_with_leading_arguments(function, leading))
}

fn arrow_with_leading_arguments(
    function: &JsFunction,
    leading: &[JsValue],
    arity: usize,
) -> Option<JsFunction> {
    let maker = LEADING_ARGUMENTS.with(|makers| {
        let mut makers = makers.borrow_mut();
        let maker = makers.entry((leading.len(), arity)).or_insert_with(|| {
            let names = |prefix: &str, count: usize| -> Vec<String> {
                (0..count).map(|index| format!("{prefix}{index}")).collect()
            };
            let (leading, arguments) = (names("l", leading.len()), names("a", arity));
            let integers = names("k", leading.len());
            let parameters = [vec!["f".to_owned()], leading.clone()].concat().join(", ");
            // The leading arguments are `usize` parameters, i32 in WebAssembly,
            // and arrive here as numbers that crossed as f64. JavaScriptCore
            // calls a WebAssembly function fast only with int32 values for
            // its i32 parameters, and a number from an f64 is not one there
            // even when it is whole: the arrow would take its slow path on
            // every call. `| 0` makes them int32 once, and is the conversion
            // the parameter makes anyway.
            let constants = integers
                .iter()
                .zip(&leading)
                .map(|(integer, leading)| format!("{integer} = {leading} | 0"))
                .collect::<Vec<_>>()
                .join(", ");
            let all = [integers, arguments.clone()].concat().join(", ");
            let body = match constants.is_empty() {
                true => format!("return ({}) => f({all});", arguments.join(", ")),
                false => format!(
                    "const {constants}; return ({}) => f({all});",
                    arguments.join(", ")
                ),
            };
            Some(
                EvaluatedFunction::new(&parameters, &body)
                    .ok()?
                    .unchecked_into(),
            )
        });
        maker.clone()
    })?;
    let mut arguments = Array::of1(function);
    arguments.extend(leading.iter().cloned());
    let made = maker.apply(&JsValue::UNDEFINED, &arguments).ok()?;
    Some(made.unchecked_into())
}

fn bound_with_leading_arguments(function: &JsFunction, leading: &[JsValue]) -> JsFunction {
    let receiver = &JsValue::UNDEFINED;
    match leading {
        [first] => function.bind1(receiver, first).unchecked_into(),
        [first, second] => function.bind2(receiver, first, second).unchecked_into(),
        _ => unreachable!("typed host functions have one or two leading arguments"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// The arrow that stands in for a bound function passes the same
    /// arguments in the same order, whatever their number and kind. Its
    /// leading arguments go to i32 parameters, so it passes them as the
    /// int32 those make of them, the address of a slot above 2 GiB too.
    #[wasm_bindgen_test]
    fn an_arrow_with_leading_arguments_calls_as_a_bound_function_does() {
        let join = JsFunction::new_with_args("...all", "return all.map(String).join(' ');");
        let leading = [JsValue::from_f64(-1.0), JsValue::from_f64(3_000_000_000.0)];
        let as_i32 = [JsValue::from(-1), JsValue::from(3_000_000_000_u32 as i32)];
        for (leading, as_i32) in [(&leading[..1], &as_i32[..1]), (&leading[..], &as_i32[..])] {
            for arity in [0, 1, 3, 26] {
                let arguments = Array::new();
                for index in 0..arity {
                    arguments.push(&match index % 3 {
                        0 => JsValue::from(index as i32),
                        1 => JsValue::from(js_sys::BigInt::from(index as i64)),
                        _ => JsValue::NULL,
                    });
                }
                let call = |function: &JsFunction| {
                    let joined = function.apply(&JsValue::UNDEFINED, &arguments);
                    joined.unwrap().as_string().unwrap()
                };
                let arrow = arrow_with_leading_arguments(&join, leading, arity);
                let arrow = call(&arrow.expect("this host makes functions from text"));
                assert_eq!(arrow, call(&bound_with_leading_arguments(&join, as_i32)));
                assert_eq!(arrow.split(' ').count(), leading.len() + arity);
                assert!(arrow.starts_with("-1"), "{arrow}");
            }
        }
    }
}

/// The arguments of a call of an async host function, as they arrived.
#[cfg(feature = "experimental-async")]
enum Arguments {
    Array(Array),
    /// On the numeric lane.
    Numbers(numeric::Numbers),
}

/// An async host function, as the hooks of its adapter need it.
#[cfg(feature = "experimental-async")]
struct AsyncHost<T, F> {
    func: F,
    env: JsFunctionEnv<T>,
    ty: FunctionType,
    store_id: wasmer_types::StoreId,
    /// For when the guest was entered by a synchronous call, which lends it
    /// the store without there being a [`StoreAsync`] to lock.
    slot: StoreSlot,
}

#[cfg(feature = "experimental-async")]
impl<T, F, Fut> AsyncHost<T, F>
where
    T: 'static,
    F: Fn(AsyncFunctionEnvMut<T>, &[Value]) -> Fut + 'static,
    Fut: Future<Output = Result<Vec<Value>, RuntimeError>> + 'static,
{
    /// What the adapter's `start` import calls, by way of `jspi`: with the
    /// arguments in an array, or, for a function of nothing but numbers, the
    /// import itself, which is called with the numbers and sets the adapter's
    /// status too.
    fn start(self: Rc<Self>, eager: bool) -> JsValue {
        // Nothing of a host function that is not eager runs in `start`: the
        // future is polled by the call, once the guest is suspended and the
        // store is free.
        let id = self.store_id;
        if numeric::is_numeric(&self.ty) {
            let has_result = !self.ty.results().is_empty();
            let arity = self.ty.params().len();
            let start = numeric::closure(arity, move |numbers| {
                let (host, args) = (Rc::clone(&self), Arguments::Numbers(numeric::copy(numbers)));
                if !eager {
                    jspi::pend(jspi::start(id, move |store| host.import(args, Some(store)))?);
                    return Ok(0.0);
                }
                let ready = jspi::start_eager(id, move |store| host.import(args, store))?;
                Ok(match ready {
                    Some(results) if has_result => numeric::result(&results),
                    _ => 0.0,
                })
            });
            start.into()
        } else if eager {
            Closure::wrap(Box::new(move |args: Array| {
                let (host, args) = (Rc::clone(&self), Arguments::Array(args));
                let ready = jspi::start_eager(id, move |store| host.import(args, store))?;
                // SAFETY: the guest is on the stack, so its store is there,
                // unless it was dropped.
                Ok(jspi::eager_results(ready, &unsafe { self.slot.store()? }))
            }) as Box<dyn Fn(Array) -> Result<JsValue, JsValue>>)
            .into_js_value()
        } else {
            Closure::wrap(Box::new(move |args: Array| {
                let (host, args) = (Rc::clone(&self), Arguments::Array(args));
                jspi::start(id, move |store| host.import(args, Some(store)))
            }) as Box<dyn Fn(Array) -> Result<i32, JsValue>>)
            .into_js_value()
        }
    }

    /// The host's half of one call of the import: the host function, and
    /// what it awaits. `store` is the store of the call the guest belongs
    /// to; without one the guest was entered synchronously, and the future
    /// is only polled inside the import call.
    fn import(self: Rc<Self>, args: Arguments, store: Option<StoreAsync>) -> jspi::ImportFuture {
        Box::pin(async move {
            let future = match store {
                Some(store) => {
                    let mut write_lock = store.write_lock().await;
                    let values = self.arguments(&mut write_lock, &args);
                    let env = self.env(AsyncFunctionEnvMutStore::Async(store));
                    let _context = StoreContext::install_async(write_lock.inner);
                    (self.func)(env, &values)
                }
                None => {
                    // SAFETY: the synchronous call beneath lent this store to
                    // the guest, as it does for a synchronous host function,
                    // unless the store was dropped.
                    let raw_store = self.slot.inner().map_err(RuntimeError::from)?;
                    let mut store = unsafe { StoreMut::from_raw(raw_store) };
                    let values = self.arguments(&mut store, &args);
                    let store = AsyncFunctionEnvMutStore::Sync(self.store_id, raw_store);
                    let env = self.env(store);
                    let _context = unsafe { StoreContext::install(raw_store) };
                    (self.func)(env, &values)
                }
            };
            let results = future.await?;
            // Checked here: values of other types would reach the guest as
            // an exception it can catch.
            let expected = self.ty.results();
            if results.len() != expected.len()
                || results.iter().zip(expected).any(|(value, ty)| value.ty() != *ty)
            {
                return Err(RuntimeError::new(
                    "an async host function returned values that do not match its signature",
                ));
            }
            Ok(results)
        })
    }

    fn arguments(&self, store: &mut impl AsStoreMut, args: &Arguments) -> Vec<Value> {
        let types = self.ty.params();
        match args {
            Arguments::Array(args) => {
                let types = types.iter().enumerate();
                types
                    .map(|(index, ty)| js_value_to_wasmer(store, ty, &args.get(index as u32)))
                    .collect()
            }
            Arguments::Numbers((numbers, arity)) => {
                numeric::values(types, &numbers[..*arity]).to_vec()
            }
        }
    }

    fn env(&self, store: AsyncFunctionEnvMutStore) -> AsyncFunctionEnvMut<T> {
        AsyncFunctionEnvMut(BackendAsyncFunctionEnvMut::Js(JsAsyncFunctionEnvMut {
            store,
            func_env: self.env.clone(),
        }))
    }
}

#[cfg(feature = "experimental-async")]
fn typed_args_from_values<Args>(
    store: &mut StoreMut,
    function_type: &FunctionType,
    values: &[Value],
) -> Result<Args, RuntimeError>
where
    Args: WasmTypeList,
{
    if values.len() != function_type.params().len() {
        return Err(RuntimeError::new(
            "typed host function received wrong number of parameters",
        ));
    }
    let mut raw_array = Args::empty_array();
    for (slot, value) in raw_array.as_mut().iter_mut().zip(values) {
        *slot = value.as_raw(store);
    }
    unsafe { Ok(Args::from_array(store, raw_array)) }
}

#[cfg(feature = "experimental-async")]
fn typed_results_to_values<Rets>(
    store: &mut StoreMut,
    function_type: &FunctionType,
    results: Rets,
) -> Result<Vec<Value>, RuntimeError>
where
    Rets: WasmTypeList,
{
    let mut raw_array = unsafe { results.into_array(store) };
    let mut values = Vec::with_capacity(function_type.results().len());
    for (raw, ty) in raw_array
        .as_mut()
        .iter()
        .zip(function_type.results())
    {
        unsafe {
            values.push(Value::from_raw(store, *ty, *raw));
        }
    }
    Ok(values)
}

impl std::fmt::Debug for Function {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.debug_struct("Function").finish()
    }
}

/// Represents a low-level Wasm static host function. See
/// `super::Function::new` and `super::Function::new_env` to learn
/// more.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct WasmFunction<Args = (), Rets = ()> {
    address: VMFunctionCallback,
    _phantom: PhantomData<(Args, Rets)>,
}

unsafe impl<Args, Rets> Send for WasmFunction<Args, Rets> {}

impl<Args, Rets> WasmFunction<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /// Creates a new `WasmFunction`.
    #[allow(dead_code)]
    pub fn new<F, T, Kind: HostFunctionKind>(function: F) -> Self
    where
        F: HostFunction<T, Args, Rets, Kind>,
        T: Sized,
    {
        Self {
            address: function.function_callback(crate::BackendKind::Js).unwrap_js(),
            _phantom: PhantomData,
        }
    }

    /// Get the function type of this `WasmFunction`.
    #[allow(dead_code)]
    pub fn ty(&self) -> FunctionType {
        FunctionType::new(Args::wasm_types(), Rets::wasm_types())
    }

    /// Get the address of this `WasmFunction`.
    #[allow(dead_code)]
    pub fn address(&self) -> VMFunctionCallback {
        self.address
    }
}

impl crate::Function {
    /// Consume [`self`] into [`crate::backend::js::function::Function`].
    pub fn into_js(self) -> crate::backend::js::function::Function {
        match self.0 {
            BackendFunction::Js(s) => s,
            _ => panic!("Not a `js` function!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::backend::js::function::Function`].
    pub fn as_js(&self) -> &crate::backend::js::function::Function {
        match self.0 {
            BackendFunction::Js(ref s) => s,
            _ => panic!("Not a `js` function!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::function::Function`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::function::Function {
        match self.0 {
            BackendFunction::Js(ref mut s) => s,
            _ => panic!("Not a `js` function!"),
        }
    }
}

macro_rules! impl_host_function {
    ([$c_struct_representation:ident] $c_struct_name:ident, $( $x:ident ),* ) => {
        paste::paste! {
        #[allow(non_snake_case)]
        pub(crate) fn [<gen_fn_callback_ $c_struct_name:lower _no_env>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList, RetsAsResult: IntoResult<Rets>, Func: Fn($( $x , )*) -> RetsAsResult + 'static>
            (this: &Func) -> crate::backend::js::vm::VMFunctionCallback {

            /// This is a function that wraps the real host
            /// function. Its address will be used inside the
            /// runtime.
            unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func>( slot: usize, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn($( $x , )*) -> RetsAsResult + 'static,
            {
                // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
                let func: &Func = unsafe { &*(&() as *const () as *const Func) };
                // SAFETY: `slot` is the address of the slot of the function's
                // store, which the function keeps alive.
                let mut store = match unsafe { crate::backend::js::store::store_at(slot) } {
                    Ok(store) => store,
                    Err(error) => wasm_bindgen::throw_val(error),
                };

                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    func($(
                        {
                            let native = unsafe { NativeWasmTypeInto::from_abi(&mut store, $x) };
                            FromToNativeWasmType::from_native(native)
                        }
                    ),* ).into_result()
                }));

                match result {
                    Ok(Ok(result)) => {
                        let c_struct = unsafe { result.into_c_struct(&mut store) };
                        return c_struct;
                    },
                    Ok(Err(trap)) => crate::backend::js::error::raise(Box::new(trap)),
                    Err(panic) => raise_host_function_panic(panic),
                }
            }

            func_wrapper::< $( $x, )* Rets, RetsAsResult, Func> as _

        }


        #[allow(non_snake_case)]
        pub(crate) fn [<gen_fn_callback_ $c_struct_name:lower>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList, RetsAsResult: IntoResult<Rets>, T: Send + 'static,  Func: Fn(FunctionEnvMut<T>, $( $x , )*) -> RetsAsResult + 'static>
            (this: &Func) -> crate::backend::js::vm::VMFunctionCallback {

            /// This is a function that wraps the real host
            /// function. Its address will be used inside the
            /// runtime.
            unsafe extern "C" fn func_wrapper<T, $( $x, )* Rets, RetsAsResult, Func>( slot: usize, handle_index: usize, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                T: Send + 'static,
                Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
            {
                // SAFETY: `slot` is the address of the slot of the function's
                // store, which the function keeps alive.
                let mut store = match unsafe { crate::backend::js::store::store_at(slot) } {
                    Ok(store) => store,
                    Err(error) => wasm_bindgen::throw_val(error),
                };

                let result = {
                    // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
                    let func: &Func = unsafe { &*(&() as *const () as *const Func) };
                    panic::catch_unwind(AssertUnwindSafe(|| {
                        // The arguments first: the environment then holds the
                        // one borrow of the store until the function returns.
                        $(
                            let $x: $x = {
                                let native = unsafe { NativeWasmTypeInto::from_abi(&mut store, $x) };
                                FromToNativeWasmType::from_native(native)
                            };
                        )*
                        let handle: crate::backend::js::store::StoreHandle<crate::backend::js::vm::VMFunctionEnvironment> =
                          unsafe {
                              crate::backend::js::store::StoreHandle::from_internal(
                                  store.objects_mut().id(),
                                  crate::backend::js::store::InternalStoreHandle::from_index(handle_index).unwrap(),
                              )
                          };
                        let env: crate::backend::js::function::env::FunctionEnvMut<T> = crate::backend::js::function::env::FunctionEnv::from_handle(handle).into_mut(&mut store);
                        func(BackendFunctionEnvMut::Js(env).into(), $( $x ),* ).into_result()
                    }))
                };

                match result {
                    Ok(Ok(result)) => {
                        let c_struct = unsafe { result.into_c_struct(&mut store) };
                        return c_struct;
                    },
                    #[allow(deprecated)]
                    #[cfg(feature = "std")]
                    Ok(Err(trap)) => crate::js::error::raise(Box::new(trap)),
                    #[cfg(feature = "core")]
                    #[allow(deprecated)]
                    Ok(Err(trap)) => crate::js::error::raise(Box::new(trap)),
                    Err(panic) => raise_host_function_panic(panic),
                }
            }

            func_wrapper::< T, $( $x, )* Rets, RetsAsResult, Func > as _
        }

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
