use crate::as_c::{param_from_c, result_to_value, type_to_c, valtype_to_type};
use crate::bindings::{
    wasm_byte_vec_new, wasm_byte_vec_new_empty, wasm_byte_vec_new_uninitialized, wasm_byte_vec_t,
    wasm_extern_as_func, wasm_func_call, wasm_func_new, wasm_func_new_with_env,
    wasm_func_param_arity, wasm_func_result_arity, wasm_func_t, wasm_func_type, wasm_functype_copy,
    wasm_functype_new, wasm_functype_params, wasm_functype_results, wasm_functype_t,
    wasm_trap_message, wasm_trap_new, wasm_trap_t, wasm_val_t, wasm_val_t__bindgen_ty_1,
    wasm_val_vec_copy, wasm_val_vec_new, wasm_val_vec_new_empty, wasm_val_vec_new_uninitialized,
    wasm_val_vec_t, wasm_valkind_enum_WASM_F32, wasm_valkind_enum_WASM_F64,
    wasm_valkind_enum_WASM_I32, wasm_valkind_enum_WASM_I64, wasm_valtype_new, wasm_valtype_t,
    wasm_valtype_vec_new, wasm_valtype_vec_new_empty, wasm_valtype_vec_t,
};
use crate::c_api::store::{InternalStoreHandle, StoreHandle};
use crate::StoreRef;
// use crate::c_api::trap::Trap;
// use crate::c_api::vm::{
//     VMExtern, VMFuncRef, VMFunction, VMFunctionCallback, VMFunctionEnvironment,
// };
use crate::c_api::bindings::wasm_func_as_extern;
use crate::c_api::vm::{
    VMExtern, VMFuncRef, VMFunction, VMFunctionCallback, VMFunctionEnvironment,
};
use crate::errors::RuntimeError;
use crate::externals::function::{HostFunction, HostFunctionKind, WithEnv, WithoutEnv};
use crate::function_env::{FunctionEnv, FunctionEnvMut};
use crate::native_type::{FromToNativeWasmType, IntoResult, NativeWasmTypeInto, WasmTypeList};
use crate::store::{AsStoreMut, AsStoreRef, StoreInner, StoreMut};
use crate::trap::Trap;
use crate::value::Value;
use std::ffi::{c_void, CStr};
use std::fmt;
use std::io::Write;
use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};
use std::ptr::null_mut;
use std::sync::Arc;

use wasmer_types::{FunctionType, RawValue};

#[derive(Clone, PartialEq)]
pub struct Function {
    pub(crate) handle: VMFunction,
}

unsafe impl Send for Function {}
unsafe impl Sync for Function {}

impl From<VMFunction> for Function {
    fn from(handle: VMFunction) -> Self {
        Self { handle }
    }
}

pub(crate) struct FunctionCallbackEnv<'a, F> {
    store: StoreMut<'a>,
    func: F,
    env_handle: Option<StoreHandle<VMFunctionEnvironment>>,
}

impl<'a, F> std::fmt::Debug for FunctionCallbackEnv<'a, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionCallbackEnv")
            .field("env_is_some", &self.env_handle.is_some())
            .finish()
    }
}

impl Function {
    /// To `VMExtern`.
    pub fn to_vm_extern(&self) -> VMExtern {
        let extern_ = unsafe { wasm_func_as_extern(self.handle) };
        assert!(
            !extern_.is_null(),
            "Returned null Function extern from wasm-c-api"
        );
        extern_
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
        let fn_ty: FunctionType = ty.into();
        let params = fn_ty.params();

        let mut param_types = params
            .into_iter()
            .map(|param| {
                let kind = type_to_c(param);
                unsafe { wasm_valtype_new(kind) }
            })
            .collect::<Vec<_>>();

        let mut wasm_param_types = unsafe {
            let mut vec = Default::default();
            wasm_valtype_vec_new(&mut vec, param_types.len(), param_types.as_ptr());
            vec
        };


        let results = fn_ty.results();
        let mut result_types = results
            .into_iter()
            .map(|param| {
                let kind = type_to_c(param);
                unsafe { wasm_valtype_new(kind) }
            })
            .collect::<Vec<_>>();

        let mut wasm_result_types = unsafe {
            let mut vec = Default::default();
            wasm_valtype_vec_new(&mut vec, result_types.len(), result_types.as_ptr());
            vec
        };


        let wasm_functype = unsafe {
            wasm_functype_new(
                &mut wasm_param_types as *mut _,
                &mut wasm_result_types as *mut _,
            )
        };

        let mut store = store.as_store_mut();
        let inner = store.inner.store.inner;

        let callback = make_fn_callback(&func);

        let mut callback_env: *mut FunctionCallbackEnv<'_, F> =
            Box::leak(Box::new(FunctionCallbackEnv {
                store,
                func,
                env_handle: Some(env.handle.clone()),
            }));

        let wasm_function = unsafe {
            wasm_func_new_with_env(
                inner,
                wasm_functype,
                Some(callback),
                callback_env as *mut _ as _,
                None,
            )
        };

        if wasm_function.is_null() {
            panic!("failed when creating new typed function");
        }

        Function {
            handle: wasm_function,
        }
    }

    /// Creates a new host `Function` from a native function.
    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let mut param_types = Args::wasm_types()
            .into_iter()
            .map(|param| {
                let kind = type_to_c(param);
                unsafe { wasm_valtype_new(kind) }
            })
            .collect::<Vec<_>>();

        let mut wasm_param_types = unsafe {
            let mut vec = Default::default();
            wasm_valtype_vec_new(&mut vec, param_types.len(), param_types.as_ptr());
            vec
        };

        let mut result_types = Rets::wasm_types()
            .into_iter()
            .map(|param| {
                let kind = type_to_c(param);
                unsafe { wasm_valtype_new(kind) }
            })
            .collect::<Vec<_>>();

        let mut wasm_result_types = unsafe {
            let mut vec = Default::default();
            wasm_valtype_vec_new(&mut vec, result_types.len(), result_types.as_ptr());
            vec
        };

        let wasm_functype =
            unsafe { wasm_functype_new(&mut wasm_param_types, &mut wasm_result_types) };

        let mut store = store.as_store_mut();
        let inner = store.inner.store.inner;

        let callback: unsafe extern "C" fn(
            *mut c_void,
            *const wasm_val_vec_t,
            *mut wasm_val_vec_t,
        ) -> *mut wasm_trap_t = unsafe { std::mem::transmute(func.function_callback()) };

        let mut callback_env: *mut FunctionCallbackEnv<'_, F> =
            Box::into_raw(Box::new(FunctionCallbackEnv {
                store,
                func,
                env_handle: None,
            }));

        let wasm_function = unsafe {
            wasm_func_new_with_env(
                inner,
                wasm_functype,
                Some(callback),
                callback_env as _,
                None,
            )
        };

        if wasm_function.is_null() {
            panic!("failed when creating new typed function");
        }

        Function {
            handle: wasm_function,
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
        T: Send + 'static,
    {
        let mut param_types = Args::wasm_types()
            .into_iter()
            .map(|param| {
                let kind = type_to_c(param);
                unsafe { wasm_valtype_new(kind) }
            })
            .collect::<Vec<_>>();

        let mut wasm_param_types = unsafe {
            let mut vec = wasm_valtype_vec_t::default();
            wasm_valtype_vec_new(&mut vec, param_types.len(), param_types.as_ptr());
            vec
        };


        let mut result_types = Rets::wasm_types()
            .into_iter()
            .map(|param| {
                let kind = type_to_c(param);
                unsafe { wasm_valtype_new(kind) }
            })
            .collect::<Vec<_>>();

        let mut wasm_result_types = unsafe {
            let mut vec: wasm_valtype_vec_t = Default::default();
            wasm_valtype_vec_new(&mut vec, result_types.len(), result_types.as_ptr());
            vec
        };

        let wasm_functype = unsafe {
            wasm_functype_new(
                &mut wasm_param_types as *mut _,
                &mut wasm_result_types as *mut _,
            )
        };

        let mut store = store.as_store_mut();
        let inner = store.inner.store.inner;

        let callback: unsafe extern "C" fn(
            *mut c_void,
            *const wasm_val_vec_t,
            *mut wasm_val_vec_t,
        ) -> *mut wasm_trap_t = unsafe { std::mem::transmute(func.function_callback()) };

        let mut callback_env: *mut FunctionCallbackEnv<'_, F> =
            Box::into_raw(Box::new(FunctionCallbackEnv {
                store,
                func,
                env_handle: Some(env.handle.clone()),
            }));

        let wasm_function = unsafe {
            wasm_func_new_with_env(
                inner,
                wasm_functype,
                Some(callback),
                callback_env as _,
                None,
            )
        };

        if wasm_function.is_null() {
            panic!("failed when creating new typed function");
        }

        Function {
            handle: wasm_function,
        }
    }

    pub fn ty(&self, _store: &impl AsStoreRef) -> FunctionType {
        let type_ = unsafe { wasm_func_type(self.handle) };
        let params: *const wasm_valtype_vec_t = unsafe { wasm_functype_params(type_) };
        let returns: *const wasm_valtype_vec_t = unsafe { wasm_functype_results(type_) };

        let params: Vec<wasmer_types::Type> = unsafe {
            let mut res = vec![];
            for i in 0..(*params).size {
                res.push(valtype_to_type(*(*params).data.wrapping_add(i)));
            }
            res
        };

        let returns: Vec<wasmer_types::Type> = unsafe {
            let mut res = vec![];
            for i in 0..(*returns).size {
                res.push(valtype_to_type(*(*returns).data.wrapping_add(i)));
            }
            res
        };

        FunctionType::new(params, returns)
    }

    pub fn call_raw(
        &self,
        _store: &mut impl AsStoreMut,
        _params: Vec<RawValue>,
    ) -> Result<Box<[Value]>, RuntimeError> {
        // There is no optimal call_raw in JSC, so we just
        // simply rely the call
        // self.call(store, params)
        unimplemented!();
    }

    pub fn call(
        &self,
        store: &mut impl AsStoreMut,
        params: &[Value],
    ) -> Result<Box<[Value]>, RuntimeError> {
        // unimplemented!();
        let store_mut = store.as_store_mut();
        // let wasm_func_param_arity(self.handle)
        let mut wasm_params = params
            .iter()
            .map(result_to_value)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let mut args = unsafe {
            let mut vec = Default::default();
            wasm_val_vec_new(&mut vec, wasm_params.len(), wasm_params.as_ptr());
            vec
        };

        std::mem::forget(wasm_params);
        let size = unsafe { wasm_func_result_arity(self.handle) };

        let mut results = unsafe {
            let mut vec = Default::default();
            wasm_val_vec_new_uninitialized(&mut vec, size);
            vec
        };

        let trap = unsafe { wasm_func_call(self.handle, &mut args, &mut results as *mut _) };

        if !trap.is_null() {
            return Err(Into::<Trap>::into(trap).into());
        }
        let results = unsafe { std::ptr::slice_from_raw_parts(results.data, results.size) };

        Ok(unsafe {
            (*results)
                .into_iter()
                .map(param_from_c)
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMFunction) -> Self {
        Self { handle: internal }
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

    /// Checks whether this `Function` can be used with the given context.
    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

fn make_fn_callback<F, T: Send + 'static>(
    func: &F,
) -> unsafe extern "C" fn(*mut c_void, *const wasm_val_vec_t, *mut wasm_val_vec_t) -> *mut wasm_trap_t
where
    F: Fn(FunctionEnvMut<'_, T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
        + 'static
        + Send
        + Sync,
{
    unsafe extern "C" fn fn_callback<F, T: Send + 'static>(
        env: *mut c_void,
        args: *const wasm_val_vec_t,
        rets: *mut wasm_val_vec_t,
    ) -> *mut wasm_trap_t
    where
        F: Fn(FunctionEnvMut<'_, T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
            + 'static
            + Send
            + Sync,
    {
        let r: *mut (FunctionCallbackEnv<'_, F>) = env as _;

        let mut store = (*r).store.as_store_mut();
        let env_handle = (*r).env_handle.as_ref().unwrap().clone();
        let mut fn_env = FunctionEnv::from_handle(env_handle).into_mut(&mut store);
        let func: &F = &(*r).func;

        let mut wasmer_args = vec![];

        for i in 0..(*args).size {
            wasmer_args.push(param_from_c(&(*(*args).data.wrapping_add(i)).clone()));
        }

        let result = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
            func(fn_env, wasmer_args.as_slice())
        }));

        match result {
            Ok(Ok(native_results)) => {
                let mut c_results: Vec<wasm_val_t> = native_results
                    .iter()
                    .map(|r| crate::as_c::result_to_value(r))
                    .collect();

                if c_results.len() != (*rets).size {
                    panic!("when calling host function: number of observed results differ from wanted results")
                }

                unsafe {
                    for i in 0..(*rets).size {
                        *((*rets).data.wrapping_add(i)) = c_results[i]
                    }
                }

                unsafe { std::ptr::null_mut() }
            }

            Ok(Err(e)) => {
                let trap: Trap = Trap::user(Box::new(e));
                unsafe { trap.into_wasm_trap(&mut store) }
            }

            Err(e) => {
                unimplemented!("host function panicked");
            }
        }
    }

    fn_callback::<F, T>
}

impl fmt::Debug for Function {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.debug_struct("Function").finish()
    }
}

macro_rules! impl_host_function {
        ( [$c_struct_representation:ident]
           $c_struct_name:ident,
           $( $x:ident ),* ) => {

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
                fn function_callback(&self) -> VMFunctionCallback {
                    /// This is a function that wraps the real host
                    /// function. Its address will be used inside the
                    /// runtime.
                    unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func, T>(env: *mut c_void, args: *const wasm_val_vec_t, results: *mut wasm_val_vec_t) -> *mut wasm_trap_t
                    where
                        $( $x: FromToNativeWasmType, )*
                        Rets: WasmTypeList,
                        RetsAsResult: IntoResult<Rets>,
                        T: Send + 'static,
                        Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
                    {

                        let r: *mut (FunctionCallbackEnv<'_, Func>) = env as _;
                        let store = &mut (*r).store.as_store_mut();

                        let mut i = 0;

                       $(
                           let c_arg = (*(*args).data.wrapping_add(i)).clone();
                           let wasmer_arg = crate::as_c::param_from_c(&c_arg);
                           let raw_arg : RawValue = wasmer_arg.as_raw(store);
                           let $x : $x = FromToNativeWasmType::from_native($x::Native::from_raw(store, raw_arg));

                           i += 1;
                        )*



                        let env_handle = (*r).env_handle.as_ref().unwrap().clone();
                        let mut fn_env = FunctionEnv::from_handle(env_handle).into_mut(store);
                        let func: &Func = &(*r).func;

                        let result = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                            ((*r).func)(fn_env, $( $x, )* ).into_result()
                        }));


                       match result {
                           Ok(Ok(result)) => {

                               let types = Rets::wasm_types();
                                let mut native_results = result.into_array(store);
                                let native_results = native_results.as_mut();

                                let native_results: Vec<Value> = native_results.into_iter().enumerate()
                                    .map(|(i, r)| Value::from_raw(store, types[i], r.clone()))
                                    .collect();

                                let mut c_results: Vec<wasm_val_t> = native_results.iter().map(|r| crate::as_c::result_to_value(r)).collect();

                                if c_results.len() != (*results).size {
                                    panic!("when calling host function: number of observed results differ from wanted results")
                                }

                                unsafe {
                                    for i in 0..(*results).size {
                                        *((*results).data.wrapping_add(i)) = c_results[i]
                                    }

                                }

                                unsafe { std::ptr::null_mut() }
                           },
                           Ok(Err(e)) => {
                                let trap: Trap =  Trap::user(Box::new(e));
                                unsafe { trap.into_wasm_trap(store) }
                           },
                           Err(e) => {
                               unimplemented!("host function panicked");
                           }
                       }


                    }

                    func_wrapper::< $( $x, )* Rets, RetsAsResult, Self, T> as VMFunctionCallback

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
                fn function_callback(&self) -> VMFunctionCallback {

                    /// This is a function that wraps the real host
                    /// function. Its address will be used inside the
                    /// runtime.
                    unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func>(env: *mut c_void, args: *const wasm_val_vec_t, results: *mut wasm_val_vec_t) -> *mut wasm_trap_t
                    where
                        $( $x: FromToNativeWasmType, )*
                        Rets: WasmTypeList,
                        RetsAsResult: IntoResult<Rets>,
                        Func: Fn($( $x , )*) -> RetsAsResult + 'static,
                    {
                        let mut r: *mut FunctionCallbackEnv<Func> = unsafe {std::mem::transmute(env)};
                        let store = &mut (*r).store.as_store_mut();
                        let mut i = 0;

                        $(
                            let c_arg = (*(*args).data.wrapping_add(i)).clone();
                            let wasmer_arg = crate::as_c::param_from_c(&c_arg);
                            let raw_arg : RawValue = wasmer_arg.as_raw(store);
                            let $x : $x = FromToNativeWasmType::from_native($x::Native::from_raw(store, raw_arg));

                            i += 1;
                        )*

                        let result = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                            ((*r).func)( $( $x, )* ).into_result()
                        }));

                        match result {
                            Ok(Ok(result)) => {

                                let types = Rets::wasm_types();
                                let mut native_results = result.into_array(store);
                                let native_results = native_results.as_mut();

                                let native_results: Vec<Value> = native_results.into_iter().enumerate()
                                    .map(|(i, r)| Value::from_raw(store, types[i], r.clone()))
                                    .collect();

                                let mut c_results: Vec<wasm_val_t> = native_results.iter().map(|r| crate::as_c::result_to_value(r)).collect();

                                if c_results.len() != (*results).size {
                                    panic!("when calling host function: number of observed results differ from wanted results")
                                }

                                unsafe {
                                    for i in 0..(*results).size {
                                        *((*results).data.wrapping_add(i)) = c_results[i]
                                    }
                                }

                                 unsafe { std::ptr::null_mut() }
                            },

                            Ok(Err(e)) => {
                                let trap: Trap =  Trap::user(Box::new(e));
                                unsafe { trap.into_wasm_trap(store) }
                                // unimplemented!("host function panicked");
                            },

                            Err(e) => {
                                unimplemented!("host function panicked");
                            }
                        }


                    }

                    func_wrapper::< $( $x, )* Rets, RetsAsResult, Self > as VMFunctionCallback
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
