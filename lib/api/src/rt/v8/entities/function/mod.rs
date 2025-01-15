//! Data types, functions and traits for `v8` runtime's `Function` implementation.
#![allow(non_snake_case)]
use std::{
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
};

use crate::{
    v8::{
        bindings::*,
        utils::convert::{IntoCApiType, IntoCApiValue, IntoWasmerType, IntoWasmerValue},
        vm::{VMFuncRef, VMFunction, VMFunctionCallback, VMFunctionEnvironment},
    },
    vm::{VMExtern, VMExternFunction},
    AsStoreMut, AsStoreRef, FromToNativeWasmType, FunctionEnv, FunctionEnvMut, IntoResult,
    NativeWasmType, NativeWasmTypeInto, RuntimeError, RuntimeFunction, RuntimeFunctionEnvMut,
    RuntimeTrap, StoreMut, Value, WasmTypeList, WithEnv, WithoutEnv,
};

use super::{super::error::Trap, check_isolate, store::StoreHandle};
use wasmer_types::{FunctionType, RawValue};

pub(crate) mod env;
pub(crate) mod typed;

pub use typed::*;

type CCallback =
    unsafe extern "C" fn(*mut c_void, *const wasm_val_t, *mut wasm_val_t) -> *mut wasm_trap_t;

#[derive(Clone, PartialEq, Eq)]
/// A WebAssembly `function` in the `v8` runtime.
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
    pub(crate) store: StoreMut<'a>,
    pub(crate) func: F,
    pub(crate) env_handle: Option<StoreHandle<VMFunctionEnvironment>>,
}

impl<'a, F> std::fmt::Debug for FunctionCallbackEnv<'a, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        VMExtern::V8(extern_)
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
        check_isolate(store);

        let mut store = store.as_store_mut();
        let fn_ty: FunctionType = ty.into();
        let params = fn_ty.params();

        let mut param_types = params
            .into_iter()
            .map(|param| {
                let kind = param.into_ct();
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
                let kind = param.into_ct();
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

        let inner = v8_store.inner;

        let callback: CCallback = make_fn_callback(&func, param_types.len());

        let mut callback_env: *mut FunctionCallbackEnv<'_, F> =
            Box::leak(Box::new(FunctionCallbackEnv {
                store,
                func,
                env_handle: Some(env.as_v8().handle.clone()),
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
        F: crate::HostFunction<(), Args, Rets, WithoutEnv> + 'static + Send + Sync,
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        check_isolate(store);
        let mut store = store.as_store_mut();

        let mut param_types = Args::wasm_types()
            .into_iter()
            .map(|param| {
                let kind = param.into_ct();
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
                let kind = param.into_ct();
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

        let inner = v8_store.inner;

        let callback: CCallback = unsafe {
            std::mem::transmute(func.function_callback(crate::RuntimeKind::V8).into_v8())
        };

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
        F: crate::HostFunction<T, Args, Rets, WithEnv>,
        Args: WasmTypeList,
        Rets: WasmTypeList,
        T: Send + 'static,
    {
        check_isolate(store);
        let mut store = store.as_store_mut();

        let mut param_types = Args::wasm_types()
            .into_iter()
            .map(|param| {
                let kind = param.into_ct();
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
                let kind = param.into_ct();
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

        let inner = v8_store.inner;

        let callback: CCallback = unsafe {
            std::mem::transmute(func.function_callback(crate::RuntimeKind::V8).into_v8())
        };

        let mut callback_env: *mut FunctionCallbackEnv<'_, F> =
            Box::into_raw(Box::new(FunctionCallbackEnv {
                store,
                func,
                env_handle: Some(env.as_v8().handle.clone()),
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

    pub fn ty(&self, store: &impl AsStoreRef) -> FunctionType {
        check_isolate(store);
        let store_ref = store.as_store_ref();
        let type_ = unsafe { wasm_func_type(self.handle) };
        let params: *const wasm_valtype_vec_t = unsafe { wasm_functype_params(type_) };
        let returns: *const wasm_valtype_vec_t = unsafe { wasm_functype_results(type_) };

        let params: Vec<wasmer_types::Type> = unsafe {
            let mut res = vec![];
            for i in 0..(*params).size {
                res.push((*(*params).data.wrapping_add(i)).into_wt());
            }
            res
        };

        let returns: Vec<wasmer_types::Type> = unsafe {
            let mut res = vec![];
            for i in 0..(*returns).size {
                res.push((*(*returns).data.wrapping_add(i)).into_wt());
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
        check_isolate(store);
        let store_mut = store.as_store_mut();

        let mut args = {
            let params = params
                .into_iter()
                .map(|v| v.clone().into_cv())
                .collect::<Vec<_>>();

            let ptr = params.as_ptr();

            std::mem::forget(params);

            ptr
        };

        let size = unsafe { wasm_func_result_arity(self.handle) };

        let mut results = {
            let mut vec = Vec::with_capacity(size);
            let ptr = vec.as_mut_ptr() as *mut wasm_val_t;

            std::mem::forget(vec);

            ptr
        };

        let mut trap;

        loop {
            trap = unsafe { wasm_func_call(self.handle, args as *const _, results) };
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
                        return Err(RuntimeError::user(trap))
                    }
                    Err(trap) => return Err(RuntimeError::user(trap)),
                }
            }
            break;
        }

        if !trap.is_null() {
            unsafe {
                let trap: Trap = trap.into();
                return Err(RuntimeError::from(trap));
            }
        }

        if !trap.is_null() {
            return Err(Into::<Trap>::into(trap).into());
        }

        unsafe {
            let results = Vec::from_raw_parts(results, size, size);

            return Ok((results)
                .into_iter()
                .map(|v| v.into_wv())
                .collect::<Vec<_>>()
                .into_boxed_slice());
        }
    }

    pub(crate) fn from_vm_extern(_store: &mut impl AsStoreMut, internal: VMExternFunction) -> Self {
        Self {
            handle: internal.into_v8(),
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

    /// Checks whether this `Function` can be used with the given context.
    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        true
    }
}

macro_rules! gen_v8_callback {
    ($args:expr) => {{
        unsafe extern "C" fn fn_callback<F, T: Send + 'static>(
            env: *mut c_void,
            args: *const wasm_val_t,
            rets: *mut wasm_val_t,
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
            let mut fn_env = crate::rt::v8::function::env::FunctionEnv::from_handle(env_handle)
                .into_mut(&mut store);
            let func: &F = &(*r).func;

            let mut wasmer_args = vec![];

            for i in 0..$args {
                wasmer_args.push((*(args).wrapping_add(i)).into_wv());
            }

            let result = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
                func(
                    FunctionEnvMut(crate::RuntimeFunctionEnvMut::V8(fn_env)),
                    wasmer_args.as_slice(),
                )
            }));

            match result {
                Ok(Ok(native_results)) => {
                    let mut c_results: Vec<wasm_val_t> =
                        native_results.into_iter().map(|r| r.into_cv()).collect();

                    unsafe {
                        for i in 0..c_results.len() {
                            *((rets).wrapping_add(i)) = c_results[i]
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
    }};
}

fn make_fn_callback<F, T: Send + 'static>(func: &F, args: usize) -> CCallback
where
    F: Fn(FunctionEnvMut<'_, T>, &[Value]) -> Result<Vec<Value>, RuntimeError>
        + 'static
        + Send
        + Sync,
{
    unsafe {
        // Hacky: v8 uses an older version of the `wasm_c_api` header in which the signature for
        // callbacks is different from that of wamr and wasmi: in v8, args (and returns) are of
        // type `*const wasm_val_t` (instead of `wasm_val_vec_t`, which also contains the length of
        // the vector), so we can't generate a single callback that at runtime gets the number of
        // arguments to perform the call to the underlying function. Therefore, we need to generate
        // different concrete callbacks for different signatures.

        seq_macro::seq!(arg_num in 1..=26 {
            let func = if args == 0 {
                gen_v8_callback!(0)
            }
        #(
             else if args == arg_num {
                gen_v8_callback!(arg_num)
             }
        )*
            else {
                panic!("v8 callbacks for function with more than 26 parameters are not supported!");
            };

        });
        return func;
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.debug_struct("Function").finish()
    }
}

impl crate::Function {
    /// Consume [`self`] into [`crate::rt::v8::function::Function`].
    pub fn into_v8(self) -> crate::rt::v8::function::Function {
        match self.0 {
            RuntimeFunction::V8(s) => s,
            _ => panic!("Not a `v8` function!"),
        }
    }

    /// Convert a reference to [`self`] into a reference to [`crate::rt::v8::function::Function`].
    pub fn as_v8(&self) -> &crate::rt::v8::function::Function {
        match self.0 {
            RuntimeFunction::V8(ref s) => s,
            _ => panic!("Not a `v8` function!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::v8::function::Function`].
    pub fn as_v8_mut(&mut self) -> &mut crate::rt::v8::function::Function {
        match self.0 {
            RuntimeFunction::V8(ref mut s) => s,
            _ => panic!("Not a `v8` function!"),
        }
    }
}

macro_rules! impl_host_function {
    ([$c_struct_representation:ident] $c_struct_name:ident, $( $x:ident ),* ) => {
        paste::paste! {
        #[allow(non_snake_case)]
        pub(crate) fn [<gen_fn_callback_ $c_struct_name:lower _no_env>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList, RetsAsResult: IntoResult<Rets>, Func: Fn($( $x , )*) -> RetsAsResult + 'static>
            (this: &Func) -> crate::rt::v8::vm::VMFunctionCallback {
            unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func>(env: *mut c_void, args: *const wasm_val_t, results: *mut wasm_val_t) -> *mut wasm_trap_t
  	        where
  	          $( $x: FromToNativeWasmType, )*
  	          Rets: WasmTypeList,
  	          RetsAsResult: IntoResult<Rets>,
  	          Func: Fn($( $x , )*) -> RetsAsResult + 'static,
              {
	        	let mut r: *mut crate::rt::v8::function::FunctionCallbackEnv<Func> = unsafe {std::mem::transmute(env)};
  	          	let store = &mut (*r).store.as_store_mut();
  	          	let mut i = 0;

                $(
	        	let c_arg = (*(args).wrapping_add(i)).clone();
  	          	let wasmer_arg = c_arg.into_wv();
  	          	let raw_arg : RawValue = wasmer_arg.as_raw(store);
  	          	let $x : $x = FromToNativeWasmType::from_native($x::Native::from_raw(store, raw_arg));
                i += 1;
                )*

                let result = panic::catch_unwind(AssertUnwindSafe(|| unsafe { ((*r).func)( $( $x, )* ).into_result() }));

                match result {
	        	  Ok(Ok(result)) => {
  	        	    let types = Rets::wasm_types();
  	        	    let size = types.len();
  	        	    let mut native_results = result.into_array(store);
  	        	    let native_results = native_results.as_mut();
  	        	    let native_results: Vec<Value> = native_results.into_iter().enumerate()
  	        	  	.map(|(i, r)| Value::from_raw(store, types[i], r.clone()))
  	        	  	.collect();
  	        	    let mut c_results: Vec<wasm_val_t> = native_results.into_iter().map(|r| r.into_cv()).collect();

  	        	    if c_results.len() != size {
  	        	  	panic!("when calling host function: number of observed results differ from wanted results")
  	        	    }

  	        	    unsafe {
  	        	  	for i in 0..size {
  	        	  	  *((results).wrapping_add(i)) = c_results[i]
  	        	  	}
  	        	    }
  	        	    unsafe { std::ptr::null_mut() }
  	        	  },
	        	  Ok(Err(e)) => {
  	        	      let trap: crate::rt::v8::error::Trap =  crate::rt::v8::error::Trap::user(Box::new(e));
  	        	      unsafe { trap.into_wasm_trap(store) }
  	        	      // unimplemented!("host function panicked");
  	        	  },
	        	  Err(e) => {
  	        	      unimplemented!("host function panicked");
  	        	  }
	        	}
	         }
  	        func_wrapper::< $( $x, )* Rets, RetsAsResult, Func> as _

        }


        #[allow(non_snake_case)]
        pub(crate) fn [<gen_fn_callback_ $c_struct_name:lower>]
            <$( $x: FromToNativeWasmType, )* Rets: WasmTypeList, RetsAsResult: IntoResult<Rets>, T: Send + 'static,  Func: Fn(FunctionEnvMut<T>, $( $x , )*) -> RetsAsResult + 'static>
            (this: &Func) -> crate::rt::v8::vm::VMFunctionCallback {

            unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func, T>(env: *mut c_void, args: *const wasm_val_t, results: *mut wasm_val_t) -> *mut wasm_trap_t
            where
	          $( $x: FromToNativeWasmType, )*
	          Rets: WasmTypeList,
	          RetsAsResult: IntoResult<Rets>,
	          T: Send + 'static,
	          Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
            {

	          let r: *mut (crate::rt::v8::function::FunctionCallbackEnv<'_, Func>) = env as _;
	          let store = &mut (*r).store.as_store_mut();
	          let mut i = 0;

              $(
	          let c_arg = (*(args).wrapping_add(i)).clone();
	          let wasmer_arg = c_arg.into_wv();
	          let raw_arg : RawValue = wasmer_arg.as_raw(store);
	          let $x : $x = FromToNativeWasmType::from_native($x::Native::from_raw(store, raw_arg));
	          i += 1;
	          )*

	          let env_handle = (*r).env_handle.as_ref().unwrap().clone();
	          let mut fn_env = crate::rt::v8::function::env::FunctionEnv::from_handle(env_handle).into_mut(store);
	          let func: &Func = &(*r).func;
	          let result = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
	              ((*r).func)(RuntimeFunctionEnvMut::V8(fn_env).into(), $( $x, )* ).into_result()
	          }));

	          match result {
  	            Ok(Ok(result)) => {
	        	  let types = Rets::wasm_types();
  	          	  let size = types.len();
  	          	  let mut native_results = result.into_array(store);
  	          	  let native_results = native_results.as_mut();

  	          	  let native_results: Vec<Value> = native_results.into_iter().enumerate()
  	          	      .map(|(i, r)| Value::from_raw(store, types[i], r.clone()))
  	          	      .collect();

  	          	  let mut c_results: Vec<wasm_val_t> = native_results.into_iter().map(|r| r.into_cv()).collect();

  	          	  if c_results.len() != size {
  	          	      panic!("when calling host function: number of observed results differ from wanted results")
  	          	  }

  	          	  unsafe {
  	          	      for i in 0..size {
  	          	          *((results).wrapping_add(i)) = c_results[i]
  	          	      }
  	          	  }

  	          	  unsafe { std::ptr::null_mut() }
  	            },
  	            Ok(Err(e)) => {
	        	  let trap: crate::rt::v8::error::Trap =  crate::rt::v8::error::Trap::user(Box::new(e));
  	              unsafe { trap.into_wasm_trap(store) }
  	            },
  	            Err(e) => { unimplemented!("host function panicked"); }
  	          }
            }

            func_wrapper::< $( $x, )* Rets, RetsAsResult, Func, T> as _

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
