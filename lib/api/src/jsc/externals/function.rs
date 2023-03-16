use crate::errors::RuntimeError;
use crate::externals::function::{HostFunction, HostFunctionKind, WithEnv, WithoutEnv};
use crate::function_env::{FunctionEnv, FunctionEnvMut};
use crate::jsc::as_js::{param_from_js, AsJs};
use crate::jsc::store::{InternalStoreHandle, StoreHandle};
use crate::jsc::vm::{
    VMExtern, VMFuncRef, VMFunction, VMFunctionBody, VMFunctionCallback, VMFunctionEnvironment,
};
use crate::native_type::{FromToNativeWasmType, IntoResult, NativeWasmTypeInto, WasmTypeList};
use crate::store::{AsStoreMut, AsStoreRef, StoreMut};
use crate::value::Value;
use std::fmt;
use std::iter::FromIterator;
use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};

use wasmer_types::{FunctionType, NativeWasmType, RawValue};

use rusty_jsc::{JSContext, JSObject, JSObjectCallAsFunctionCallback, JSValue};

#[inline]
fn result_to_js(val: &Value) -> JSValue {
    unimplemented!();
    // match val {
    //     Value::I32(i) => JSValue::from_f64(*i as _),
    //     Value::I64(i) => JSValue::from_f64(*i as _),
    //     Value::F32(f) => JSValue::from_f64(*f as _),
    //     Value::F64(f) => JSValue::from_f64(*f),
    //     Value::V128(f) => JSValue::from_f64(*f as _),
    //     val => unimplemented!(
    //         "The value `{:?}` is not yet supported in the JS Function API",
    //         val
    //     ),
    // }
}

#[inline]
fn results_to_js_array(values: &[Value]) -> JSValue {
    unimplemented!();
    // JSValue::from_iter(values.iter().map(result_to_js))
}

#[derive(Clone, PartialEq)]
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
    pub fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Function(self.handle.clone())
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
        unimplemented!();
        // let mut store = store.as_store_mut();
        // let function_type = ty.into();
        // let func_ty = function_type.clone();
        // let raw_store = store.as_raw() as *mut u8;
        // let raw_env = env.clone();
        // let wrapped_func: JSValue = match function_type.results().len() {
        //     0 => Closure::wrap(Box::new(move |args: &JSValue| {
        //         let mut store: StoreMut = unsafe { StoreMut::from_raw(raw_store as _) };
        //         let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
        //         let wasm_arguments = function_type
        //             .params()
        //             .iter()
        //             .enumerate()
        //             .map(|(i, param)| param_from_js(param, &args.get(i as u32)))
        //             .collect::<Vec<_>>();
        //         let _results = func(env, &wasm_arguments)?;
        //         Ok(())
        //     })
        //         as Box<dyn FnMut(&Array) -> Result<(), JSValue>>)
        //     .into_js_value(),
        //     1 => Closure::wrap(Box::new(move |args: &Array| {
        //         let mut store: StoreMut = unsafe { StoreMut::from_raw(raw_store as _) };
        //         let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
        //         let wasm_arguments = function_type
        //             .params()
        //             .iter()
        //             .enumerate()
        //             .map(|(i, param)| param_from_js(param, &args.get(i as u32)))
        //             .collect::<Vec<_>>();
        //         let results = func(env, &wasm_arguments)?;
        //         return Ok(result_to_js(&results[0]));
        //     })
        //         as Box<dyn FnMut(&Array) -> Result<JSValue, JSValue>>)
        //     .into_js_value(),
        //     _n => Closure::wrap(Box::new(move |args: &Array| {
        //         let mut store: StoreMut = unsafe { StoreMut::from_raw(raw_store as _) };
        //         let env: FunctionEnvMut<T> = raw_env.clone().into_mut(&mut store);
        //         let wasm_arguments = function_type
        //             .params()
        //             .iter()
        //             .enumerate()
        //             .map(|(i, param)| param_from_js(param, &args.get(i as u32)))
        //             .collect::<Vec<_>>();
        //         let results = func(env, &wasm_arguments)?;
        //         return Ok(results_to_js_array(&results));
        //     })
        //         as Box<dyn FnMut(&Array) -> Result<Array, JSValue>>)
        //     .into_js_value(),
        // };

        // let dyn_func =
        //     JSFunction::new_with_args("f", "return f(Array.prototype.slice.call(arguments, 1))");
        // let binded_func = dyn_func.bind1(&JSValue::UNDEFINED, &wrapped_func);
        // let vm_function = VMFunction::new(binded_func, func_ty);
        // Self::from_vm_extern(&mut store, vm_function)
    }

    /// Creates a new host `Function` from a native function.
    pub fn new_typed<F, Args, Rets>(store: &mut impl AsStoreMut, func: F) -> Self
    where
        F: HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        // unimplemented!();
        let store = store.as_store_mut();
        if std::mem::size_of::<F>() != 0 {
            Self::closures_unsupported_panic();
        }
        let function = WasmFunction::<Args, Rets>::new(func);
        let callback = function.callback(store.engine().0.context());

        // let binded_func = func.bind1(
        //     &JSValue::UNDEFINED,
        //     &JSValue::from_f64(store.as_raw() as *mut u8 as usize as f64),
        // );
        let ty = function.ty();
        let vm_function = VMFunction::new(callback, ty);
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
        unimplemented!();
        // let store = store.as_store_mut();
        // if std::mem::size_of::<F>() != 0 {
        //     Self::closures_unsupported_panic();
        // }
        // let function = WasmFunction::<Args, Rets>::new(func);
        // let address = function.address() as usize as u32;

        // let ft = wasm_bindgen::function_table();
        // let as_table = ft.unchecked_ref::<js_sys::WebAssembly::Table>();
        // let func = as_table.get(address).unwrap();

        // let binded_func = func.bind2(
        //     &JSValue::UNDEFINED,
        //     &JSValue::from_f64(store.as_raw() as *mut u8 as usize as f64),
        //     &JSValue::from_f64(env.handle.internal_handle().index() as f64),
        // );
        // let ty = function.ty();
        // let vm_function = VMFunction::new(binded_func, ty);
        // Self {
        //     handle: vm_function,
        // }
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
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.0.context();
        let params_list = params
            .iter()
            .map(|v| v.as_jsvalue(&store_mut))
            .collect::<Vec<_>>();
        let result = {
            let mut r;
            // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
            loop {
                let store_mut = store.as_store_mut();
                let engine = store_mut.engine();
                let context = engine.0.context();
                r = self.handle.function.call(
                    &context,
                    JSValue::undefined(&context).to_object(&context),
                    &params_list,
                );
                if let Some(callback) = store_mut.inner.on_called.take() {
                    match callback(store_mut) {
                        Ok(wasmer_types::OnCalledAction::InvokeAgain) => {
                            continue;
                        }
                        Ok(wasmer_types::OnCalledAction::Finish) => {
                            break;
                        }
                        Ok(wasmer_types::OnCalledAction::Trap(trap)) => {
                            return Err(RuntimeError::user(trap))
                        }
                        Err(trap) => return Err(RuntimeError::user(trap)),
                    }
                }
                break;
            }
            r?
        };
        let result_types = self.handle.ty.results();
        let store_mut = store.as_store_mut();
        let engine = store_mut.engine();
        let context = engine.0.context();
        match result_types.len() {
            0 => Ok(Box::new([])),
            1 => {
                let value = param_from_js(&context, &result_types[0], &result);
                Ok(vec![value].into_boxed_slice())
            }
            n => {
                let result = result.to_object(&context);
                Ok((0..n)
                    .map(|i| {
                        let js_val = result.get_property_at_index(&context, i as _).unwrap();
                        param_from_js(&context, &result_types[i], &js_val)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice())
            }
        }
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

    #[track_caller]
    fn closures_unsupported_panic() -> ! {
        unimplemented!("Closures (functions with captured environments) are currently unsupported with native functions. See: https://github.com/wasmerio/wasmer/issues/1840")
    }

    /// Checks whether this `Function` can be used with the given context.
    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

impl fmt::Debug for Function {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.debug_struct("Function").finish()
    }
}

/// Represents a low-level Wasm static host function. See
/// `super::Function::new` and `super::Function::new_env` to learn
/// more.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct WasmFunction<Args = (), Rets = ()> {
    callback: JSObjectCallAsFunctionCallback,
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
            callback: function.function_callback(),
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
    pub fn callback(&self, context: &JSContext) -> JSObject {
        JSObject::new_function_with_callback(context, "FunctionCallback".to_string(), self.callback)
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
                    unimplemented!();
                    // /// This is a function that wraps the real host
                    // /// function. Its address will be used inside the
                    // /// runtime.
                    // unsafe extern "C" fn func_wrapper<T, $( $x, )* Rets, RetsAsResult, Func>( store_ptr: usize, handle_index: usize, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
                    // where
                    //     $( $x: FromToNativeWasmType, )*
                    //     Rets: WasmTypeList,
                    //     RetsAsResult: IntoResult<Rets>,
                    //     T: Send + 'static,
                    //     Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
                    // {
                    //     let mut store = StoreMut::from_raw(store_ptr as *mut _);
                    //     let mut store2 = StoreMut::from_raw(store_ptr as *mut _);

                    //     let result = {
                    //         // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
                    //         let func: &Func = &*(&() as *const () as *const Func);
                    //         panic::catch_unwind(AssertUnwindSafe(|| {
                    //             let handle: StoreHandle<VMFunctionEnvironment> = StoreHandle::from_internal(store2.objects_mut().id(), InternalStoreHandle::from_index(handle_index).unwrap());
                    //             let env: FunctionEnvMut<T> = FunctionEnv::from_handle(handle).into_mut(&mut store2);
                    //             func(env, $( FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x)) ),* ).into_result()
                    //         }))
                    //     };

                    //     match result {
                    //         Ok(Ok(result)) => return result.into_c_struct(&mut store),
                    //         #[allow(deprecated)]
                    //         #[cfg(feature = "std")]
                    //         Ok(Err(trap)) => RuntimeError::raise(Box::new(trap)),
                    //         #[cfg(feature = "core")]
                    //         #[allow(deprecated)]
                    //         Ok(Err(trap)) => RuntimeError::raise(Box::new(trap)),
                    //         Err(_panic) => unimplemented!(),
                    //     }
                    // }

                    // func_wrapper::< T, $( $x, )* Rets, RetsAsResult, Self > as *const VMFunctionBody
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
                    use rusty_jsc::{JSContext, JSObject, JSValue, callback};

                    #[callback]
                    fn fn_callback<$( $x, )* Rets, RetsAsResult, Func>(
                        ctx: JSContext,
                        function: JSObject,
                        this_object: JSObject,
                        arguments: &[JSValue],
                    ) -> Result<JSValue, JSValue>
                    where
                        $( $x: FromToNativeWasmType, )*
                        Rets: WasmTypeList,
                        RetsAsResult: IntoResult<Rets>,
                        Func: Fn($( $x , )*) -> RetsAsResult + 'static,
                        // $( $x: NativeWasmTypeInto, )*
                    {
                        use std::convert::TryInto;
                        // dbg!(arguments.len());
                        // dbg!(arguments[0].to_object(&ctx).get_property(&ctx, "prototype".into()).to_string(&ctx));
                        // dbg!(arguments[0].to_number(&ctx) as usize);
                        println!("CALLING 0");

                        let func: &Func = &*(&() as *const () as *const Func);
                        let global = ctx.get_global_object();
                        let store_ptr = global.get_property(&ctx, "__store_ptr".to_string()).to_number(&ctx);

                        let mut store = StoreMut::from_raw(store_ptr as usize as *mut _);
                        println!("CALLING 1");
                        let result = panic::catch_unwind(AssertUnwindSafe(|| {
                            // let list =
                            type JSArray<'a> = &'a [JSValue; count_idents!( $( $x ),* )];
                            // println!("CALLING 1.1 {}, idents+1: {}, idents: {}", arguments.len(), count_idents_plus_one!( $( $x ),* ), count_idents!( $( $x ),* ));
                            let args_without_store: JSArray = arguments.try_into().unwrap();
                            println!("CALLING 1.2");
                            let [ $( $x ),* ] = args_without_store;
                            println!("CALLING 2");
                            // let ABI = <$x::Native as NativeWasmType>::Abi
                            // let r: ($( $x , )*) = ($( $x::from_raw(&mut store, RawValue { i32: $x.to_number(&ctx) as _ }) ),*);
                            // func($( FromToNativeWasmType::from_native($x.to_number(&ctx) as <$x::Native as NativeWasmType>::Abi) ),* ).into_result()
                            func($( FromToNativeWasmType::from_native( $x::Native::from_raw(&mut store, RawValue { u128: {
                                // TODO: This may not be the fastest way, but JSC doesn't expose a BigInt interface
                                // so the only thing we can do is parse from the string repr
                                if $x.is_number(&ctx) {
                                    $x.to_number(&ctx) as _
                                }
                                else {
                                    $x.to_string(&ctx).parse::<u128>().unwrap()
                                }
                            } }) ) ),* ).into_result()
                        }));
                        println!("CALLING 3");

                        // println!("Result {:?}", result.unwrap().unwrap().into_c_struct(&mut store));
                        // println!("Result {}", result.unwrap().unwrap().into_array(&mut store));


                        match result {
                            Ok(Ok(result)) => {
                                println!("RESULT");
                                match Rets::size() {
                                    0 => {Ok(JSValue::undefined(&ctx))},
                                    1 => {
                                        // unimplemented!();

                                        let ty = Rets::wasm_types()[0];
                                        let mut arr = result.into_array(&mut store);
                                        // Value::from_raw(&store, ty, arr[0])
                                        let val = Value::from_raw(&mut store, ty, arr.as_mut()[0]);
                                        println!("RETURNED: {:?}", val);
                                        let value: JSValue = val.as_jsvalue(&store);
                                        println!("AS JS");
                                        Ok(value)
                                        // *mut_rets = val.as_raw(&mut store);
                                    }
                                    _n => {
                                        // if !results.is_array(&context) {
                                        //     panic!("Expected results to be an array.")
                                        // }
                                        let mut arr = result.into_array(&mut store);
                                        let result_values = Rets::wasm_types().iter().enumerate().map(|(i, ret_type)| {
                                            let raw = arr.as_mut()[i];
                                            Value::from_raw(&mut store, *ret_type, raw).as_jsvalue(&mut store)
                                        }).collect::<Vec<_>>();
                                        Ok(JSObject::new_array(&ctx, &result_values).to_jsvalue())
                                    }
                                }
                            },
                            #[cfg(feature = "std")]
                            #[allow(deprecated)]
                            Ok(Err(trap)) => {
                                Err(JSValue::string(&ctx, format!("{:?}", trap)).unwrap())
                                // RuntimeError::raise(Box::new(trap))
                            },
                            #[cfg(feature = "core")]
                            #[allow(deprecated)]
                            Ok(Err(trap)) => {
                                Err(JSValue::string(&ctx, format!("{:?}", trap)).unwrap())
                                // RuntimeError::raise(Box::new(trap))
                            },
                            Err(panic) => {
                                Err(JSValue::string(&ctx, format!("panic: {:?}", panic)).unwrap())
                                // We can't just resume the unwind, because it will put
                                // JavacriptCore in a bad state, so we need to transform
                                // the error

                                // std::panic::resume_unwind(panic)
                            },
                        }

                    }
                    Some(fn_callback::< $( $x, )* Rets, RetsAsResult, Self > as _)
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
