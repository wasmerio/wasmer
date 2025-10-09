pub(crate) mod env;
pub(crate) mod typed;
use std::marker::PhantomData;

pub(crate) use typed::*;

use js_sys::{Array, Function as JsFunction};
use wasm_bindgen::{JsCast, prelude::*};
use wasmer_types::{FunctionType, RawValue};

use crate::{
    AsStoreMut, AsStoreRef, BackendFunction, BackendFunctionEnv, BackendFunctionEnvMut,
    FromToNativeWasmType, FunctionEnv, FunctionEnvMut, HostFunction, HostFunctionKind, IntoResult,
    NativeWasmType, NativeWasmTypeInto, RuntimeError, StoreMut, Value, WasmTypeList, WithEnv,
    WithoutEnv,
    js::{
        utils::convert::{AsJs as _, js_value_to_wasmer, wasmer_value_to_js},
        vm::{VMFuncRef, VMFunctionCallback, function::VMFunction},
    },
    vm::{VMExtern, VMExternFunction},
};

use std::panic::{self, AssertUnwindSafe};

#[inline]
fn wasmer_array_to_js_array(values: &[Value]) -> Array {
    Array::from_iter(values.iter().map(wasmer_value_to_js))
}

#[derive(Clone, PartialEq, Eq)]
pub struct Function {
    pub(crate) handle: VMFunction,
}

// Function can't be Send in js because it dosen't support `structuredClone`
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
                    .map(|(i, param)| js_value_to_wasmer(param, &args.get(i as u32)))
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
                    .map(|(i, param)| js_value_to_wasmer(param, &args.get(i as u32)))
                    .collect::<Vec<_>>();
                let results = func(env, &wasm_arguments)?;
                Ok(wasmer_value_to_js(&results[0]))
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
                    .map(|(i, param)| js_value_to_wasmer(param, &args.get(i as u32)))
                    .collect::<Vec<_>>();
                let results = func(env, &wasm_arguments)?;
                Ok(wasmer_array_to_js_array(&results))
            })
                as Box<dyn FnMut(&Array) -> Result<Array, JsValue>>)
            .into_js_value(),
        };

        let dyn_func =
            JsFunction::new_with_args("f", "return f(Array.prototype.slice.call(arguments, 1))");
        let binded_func = dyn_func.bind1(&JsValue::UNDEFINED, &wrapped_func);
        let vm_function = VMFunction::new(binded_func, func_ty);
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

        let binded_func = func.bind1(
            &JsValue::UNDEFINED,
            &JsValue::from_f64(store.as_raw() as *mut u8 as usize as f64),
        );
        let ty = function.ty();
        let vm_function = VMFunction::new(binded_func, ty);
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

        let binded_func = func.bind2(
            &JsValue::UNDEFINED,
            &JsValue::from_f64(store.as_raw() as *mut u8 as usize as f64),
            &JsValue::from_f64(env.as_js().handle.internal_handle().index() as f64),
        );
        let ty = function.ty();
        let vm_function = VMFunction::new(binded_func, ty);
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
        // Annotation is here to prevent spurious IDE warnings.
        let arr = js_sys::Array::new_with_length(params.len() as u32);

        // let raw_env = env.as_raw() as *mut u8;
        // let mut env = unsafe { FunctionEnvMut::from_raw(raw_env as *mut StoreInner<()>) };

        for (i, param) in params.iter().enumerate() {
            let js_value = param.as_jsvalue(&store.as_store_ref());
            arr.set(i as u32, js_value);
        }

        let result = {
            let mut r;
            // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
            loop {
                r = js_sys::Reflect::apply(
                    &self.handle.function,
                    &wasm_bindgen::JsValue::NULL,
                    &arr,
                );
                let store_mut = store.as_store_mut();
                if let Some(callback) = store_mut.inner.on_called.take() {
                    match callback(store_mut) {
                        Ok(wasmer_types::OnCalledAction::InvokeAgain) => {
                            continue;
                        }
                        Ok(wasmer_types::OnCalledAction::Finish) => {
                            break;
                        }
                        Ok(wasmer_types::OnCalledAction::Trap(trap)) => {
                            return Err(RuntimeError::user(trap));
                        }
                        Err(trap) => return Err(RuntimeError::user(trap)),
                    }
                }
                break;
            }
            r?
        };

        let result_types = self.handle.ty.results();
        match result_types.len() {
            0 => Ok(Box::new([])),
            1 => {
                let value = js_value_to_wasmer(&result_types[0], &result);
                Ok(vec![value].into_boxed_slice())
            }
            _n => {
                let result_array: Array = result.into();
                Ok(result_array
                    .iter()
                    .enumerate()
                    .map(|(i, js_val)| js_value_to_wasmer(&result_types[i], &js_val))
                    .collect::<Vec<_>>()
                    .into_boxed_slice())
            }
        }
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMExternFunction) -> Self {
        Self {
            handle: internal.into_js(),
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
    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
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
            address: function.function_callback(crate::BackendKind::Js).into_js(),
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
            unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func>( store_ptr: usize, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                Func: Fn($( $x , )*) -> RetsAsResult + 'static,
            {
                // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
                let func: &Func = unsafe { &*(&() as *const () as *const Func) };
                let mut store = unsafe { StoreMut::from_raw(store_ptr as *mut _) };

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
                    #[cfg(feature = "std")]
                    #[allow(deprecated)]
                    Ok(Err(trap)) => crate::backend::js::error::raise(Box::new(trap)),
                    #[cfg(feature = "core")]
                    #[allow(deprecated)]
                    Ok(Err(trap)) => crate::backend::js::error::raise(Box::new(trap)),
                    Err(_panic) => unimplemented!(),
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
            unsafe extern "C" fn func_wrapper<T, $( $x, )* Rets, RetsAsResult, Func>( store_ptr: usize, handle_index: usize, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
            where
                $( $x: FromToNativeWasmType, )*
                Rets: WasmTypeList,
                RetsAsResult: IntoResult<Rets>,
                T: Send + 'static,
                Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
            {
                let mut store = unsafe { StoreMut::from_raw(store_ptr as *mut _) };
                let mut store2 = unsafe { StoreMut::from_raw(store_ptr as *mut _) };

                let result = {
                    // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
                    let func: &Func = unsafe { &*(&() as *const () as *const Func) };
                    panic::catch_unwind(AssertUnwindSafe(|| {
                        let handle: crate::backend::js::store::StoreHandle<crate::backend::js::vm::VMFunctionEnvironment> =
                          unsafe {
                              crate::backend::js::store::StoreHandle::from_internal(
                                  store2.objects_mut().id(),
                                  crate::backend::js::store::InternalStoreHandle::from_index(handle_index).unwrap(),
                              )
                          };
                        let env: crate::backend::js::function::env::FunctionEnvMut<T> = crate::backend::js::function::env::FunctionEnv::from_handle(handle).into_mut(&mut store2);
                        func(BackendFunctionEnvMut::Js(env).into(), $(
                            {
                                let native = unsafe { NativeWasmTypeInto::from_abi(&mut store, $x) };
                                FromToNativeWasmType::from_native(native)
                            }
                        ),* ).into_result()
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
                    Err(_panic) => unimplemented!(),
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
