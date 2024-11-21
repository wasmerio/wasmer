use crate::{
    utils::{FromToNativeWasmType, IntoResult, NativeWasmTypeInto, WasmTypeList},
    AsStoreMut, AsStoreRef, FunctionEnvMut, FunctionType, HostFunction, RuntimeError,
    RuntimeFunctionEnv, RuntimeFunctionEnvMut, StoreInner, StoreMut, StoreRef, Value, WithEnv,
    WithoutEnv,
};

use std::panic::{self, AssertUnwindSafe};
use std::{cell::UnsafeCell, cmp::max, ffi::c_void};
use wasmer_types::{NativeWasmType, RawValue};

macro_rules! impl_host_function {
    ([$c_struct_representation:ident] $c_struct_name:ident, $( $x:ident ),* ) => {
#[allow(unused_parens)]
impl< $( $x, )* Rets, RetsAsResult, T, Func> crate::HostFunction<T, ( $( $x ),* ), Rets, WithEnv> for Func where
    $( $x: FromToNativeWasmType, )*
    Rets: WasmTypeList,
    RetsAsResult: IntoResult<Rets>,
    T: Send + 'static,
    Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
{
  #[cfg(feature = "jsc")]
  #[allow(non_snake_case)]
  fn jsc_function_callback(&self) -> crate::rt::jsc::vm::VMFunctionCallback {
    use crate::rt::jsc::{utils::convert::AsJsc, store::{StoreHandle,  InternalStoreHandle}, vm::VMFunctionEnvironment};
    use rusty_jsc::{JSObject, JSValue, callback};


     #[callback]
     fn fn_callback<T, $( $x, )* Rets, RetsAsResult, Func>(
         ctx: JSContext,
         function: JSObject,
         this_object: JSObject,
         arguments: &[JSValue],
     ) -> Result<JSValue, JSValue>
     where
         $( $x: FromToNativeWasmType, )*
         Rets: WasmTypeList,
         RetsAsResult: IntoResult<Rets>,
         Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
         T: Send + 'static,
     {
         use std::convert::TryInto;

         let func: &Func = &*(&() as *const () as *const Func);
         let global = ctx.get_global_object();
         let store_ptr = global.get_property(&ctx, "__store_ptr".to_string()).to_number(&ctx).unwrap();
         if store_ptr.is_nan() {
             panic!("Store pointer is invalid. Received {}", store_ptr as usize)
         }
         let mut store = StoreMut::from_raw(store_ptr as usize as *mut _);

         let handle_index = arguments[0].to_number(&ctx).unwrap() as usize;
         let handle: StoreHandle<VMFunctionEnvironment> = StoreHandle::from_internal(store.objects_mut().id(), InternalStoreHandle::from_index(handle_index).unwrap());
         let env = crate::rt::jsc::function::env::FunctionEnv::from_handle(handle).into_mut(&mut store);

         let result = panic::catch_unwind(AssertUnwindSafe(|| {
             type JSArray<'a> = &'a [JSValue; count_idents!( $( $x ),* )];
             let args_without_store: JSArray = arguments[1..].try_into().unwrap();
             let [ $( $x ),* ] = args_without_store;
             let mut store = StoreMut::from_raw(store_ptr as usize as *mut _);
             func(RuntimeFunctionEnvMut::Jsc(env).into(), $( FromToNativeWasmType::from_native( $x::Native::from_raw(&mut store, RawValue { u128: {
                 // TODO: This may not be the fastest way, but JSC doesn't expose a BigInt interface
                 // so the only thing we can do is parse from the string repr
                 if $x.is_number(&ctx) {
                     $x.to_number(&ctx).unwrap() as _
                 }
                 else {
                     $x.to_string(&ctx).unwrap().to_string().parse::<u128>().unwrap()
                 }
             } }) ) ),* ).into_result()
         }));

         match result {
             Ok(Ok(result)) => {
                 match Rets::size() {
                     0 => {Ok(JSValue::undefined(&ctx))},
                     1 => {
                         // unimplemented!();

                         let ty = Rets::wasm_types()[0];
                         let mut arr = result.into_array(&mut store);
                         // Value::from_raw(&store, ty, arr[0])
                         let val = Value::from_raw(&mut store, ty, arr.as_mut()[0]);
                         let value: JSValue = val.as_jsc_value(&store);
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
                             Value::from_raw(&mut store, *ret_type, raw).as_jsc_value(&mut store)
                         }).collect::<Vec<_>>();
                         Ok(JSObject::new_array(&ctx, &result_values).unwrap().to_jsvalue())
                     }
                 }
             },
             #[cfg(feature = "std")]
             Ok(Err(err)) => {
                 let trap = crate::jsc::vm::Trap::user(Box::new(err));
                 Err(trap.into_jsc_value(&ctx))
             },
             #[cfg(feature = "core")]
             Ok(Err(err)) => {
                 let trap = crate::jsc::vm::Trap::user(Box::new(err));
                 Err(trap.into_jsc_value(&ctx))
             },
             Err(panic) => {
                 Err(JSValue::string(&ctx, format!("panic: {:?}", panic)))
             },
         }

     }

     Some(fn_callback::<T, $( $x, )* Rets, RetsAsResult, Self > as _)
   }

  #[cfg(feature = "js")]
  #[allow(non_snake_case)]
  fn js_function_callback(&self) -> crate::rt::js::vm::VMFunctionCallback {
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
          let mut store = StoreMut::from_raw(store_ptr as *mut _);
          let mut store2 = StoreMut::from_raw(store_ptr as *mut _);

          let result = {
              // let env: &Env = unsafe { &*(ptr as *const u8 as *const Env) };
              let func: &Func = &*(&() as *const () as *const Func);
              panic::catch_unwind(AssertUnwindSafe(|| {
                  let handle: crate::rt::js::store::StoreHandle<crate::rt::js::vm::VMFunctionEnvironment> =
                    crate::rt::js::store::StoreHandle::from_internal(store2.objects_mut().id(), crate::rt::js::store::InternalStoreHandle::from_index(handle_index).unwrap());
                  let env: crate::rt::js::function::env::FunctionEnvMut<T> = crate::rt::js::function::env::FunctionEnv::from_handle(handle).into_mut(&mut store2);
                  func(RuntimeFunctionEnvMut::Js(env).into(), $( FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x)) ),* ).into_result()
              }))
          };

          match result {
              Ok(Ok(result)) => return result.into_c_struct(&mut store),
              #[allow(deprecated)]
              #[cfg(feature = "std")]
              Ok(Err(trap)) => crate::js::error::raise(Box::new(trap)),
              #[cfg(feature = "core")]
              #[allow(deprecated)]
              Ok(Err(trap)) => crate::js::error::raise(Box::new(trap)),
              Err(_panic) => unimplemented!(),
          }
      }

      func_wrapper::< T, $( $x, )* Rets, RetsAsResult, Self > as _
  }

  #[cfg(feature = "wamr")]
  #[allow(non_snake_case)]
  fn wamr_function_callback(&self) -> crate::rt::wamr::vm::VMFunctionCallback {
	use crate::rt::wamr::bindings::*;
    use crate::rt::wamr::utils::convert::*;

	unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func, T>(env: *mut c_void, args: *const wasm_val_vec_t, results: *mut wasm_val_vec_t) -> *mut wasm_trap_t
	where
	  $( $x: FromToNativeWasmType, )*
	  Rets: WasmTypeList,
	  RetsAsResult: IntoResult<Rets>,
	  T: Send + 'static,
	  Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
    {

	  let r: *mut (crate::rt::wamr::function::FunctionCallbackEnv<'_, Func>) = env as _;
	  let store = &mut (*r).store.as_store_mut();

	  let mut i = 0;

	  $(
	  let c_arg = (*(*args).data.wrapping_add(i)).clone();
	  let wasmer_arg = c_arg.into_wv();
	  let raw_arg : RawValue = wasmer_arg.as_raw(store);
	  let $x : $x = FromToNativeWasmType::from_native($x::Native::from_raw(store, raw_arg));

	  i += 1;
      )*

	  let env_handle = (*r).env_handle.as_ref().unwrap().clone();
	  let mut fn_env = crate::rt::wamr::function::env::FunctionEnv::from_handle(env_handle).into_mut(store);
	  let func: &Func = &(*r).func;

	  let result = panic::catch_unwind(AssertUnwindSafe(|| unsafe {
	      ((*r).func)(RuntimeFunctionEnvMut::Wamr(fn_env).into(), $( $x, )* ).into_result()
	  }));


	  match result {
	      Ok(Ok(result)) => {
	  	  let types = Rets::wasm_types();
	  	  let mut native_results = result.into_array(store);
	  	  let native_results = native_results.as_mut();

	  	  let native_results: Vec<Value> = native_results.into_iter().enumerate().map(|(i, r)| Value::from_raw(store, types[i], r.clone())).collect();

	  	  let mut c_results: Vec<wasm_val_t> = native_results.into_iter().map(IntoCApiValue::into_cv).collect();

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

	    Ok(Err(e)) => { let trap = crate::rt::wamr::error::Trap::user(Box::new(e)); unsafe { trap.into_wasm_trap(store) } },

	    Err(e) => { unimplemented!("host function panicked"); }
	  }
	}

	func_wrapper::< $( $x, )* Rets, RetsAsResult, Self, T> as _

  }

  #[cfg(feature = "v8")]
  #[allow(non_snake_case)]
  fn v8_function_callback(&self) -> crate::rt::v8::vm::VMFunctionCallback {
	use crate::rt::v8::bindings::*;
    use crate::rt::v8::utils::convert::*;
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

    func_wrapper::< $( $x, )* Rets, RetsAsResult, Self, T> as _
  }

  #[cfg(feature = "sys")]
  #[allow(non_snake_case)]
  fn sys_function_callback(&self) -> crate::rt::sys::vm::VMFunctionCallback {
    unsafe extern "C" fn func_wrapper<T: Send + 'static, $( $x, )* Rets, RetsAsResult, Func>( env: &crate::rt::sys::function::StaticFunction<Func, T>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
    where
  	$( $x: FromToNativeWasmType, )*
  	Rets: WasmTypeList,
  	RetsAsResult: IntoResult<Rets>,
  	Func: Fn(FunctionEnvMut<T>, $( $x , )*) -> RetsAsResult + 'static,
    {
  	let mut store = StoreMut::from_raw(env.raw_store as *mut _);
  	let result = wasmer_vm::on_host_stack(|| {
  	    panic::catch_unwind(AssertUnwindSafe(|| {
  	        $(
  	            let $x = FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x));
  	        )*
  	        let store_mut = StoreMut::from_raw(env.raw_store as *mut _);
  	        let f_env = crate::rt::sys::function::env::FunctionEnvMut {
  	            store_mut,
  	            func_env: env.env.as_sys().clone(),
  	        }.into();
  	        (env.func)(f_env, $($x),* ).into_result()
  	    }))
  	});

  	match result {
  	    Ok(Ok(result)) => return result.into_c_struct(&mut store),
  	    Ok(Err(trap)) => wasmer_vm::raise_user_trap(Box::new(trap)),
  	    Err(panic) => wasmer_vm::resume_panic(panic) ,
  	}
    }
    func_wrapper::< T, $( $x, )* Rets, RetsAsResult, Self > as _
  }

  #[cfg(feature = "sys")]
  #[allow(non_snake_case)]
  fn sys_call_trampoline_address() -> crate::rt::sys::vm::VMTrampoline {
    unsafe extern "C" fn call_trampoline<$( $x: FromToNativeWasmType, )* Rets: WasmTypeList>
  	(
          vmctx: *mut crate::rt::sys::vm::VMContext,
          body: crate::rt::sys::vm::VMFunctionCallback,
          args: *mut RawValue,
      ) {
	 let body: unsafe extern "C" fn(vmctx: *mut crate::rt::sys::vm::VMContext, $( $x: <$x::Native as NativeWasmType>::Abi, )*) -> Rets::CStruct = std::mem::transmute(body);
  	 let mut _n = 0;
  	 $(
  	     let $x = *args.add(_n).cast();
  	     _n += 1;
  	 )*

  	 let results = body(vmctx, $( $x ),*);
  	 Rets::write_c_struct_to_ptr(results, args);
    }
	 call_trampoline::<$( $x, )* Rets> as _
  }
}

// Implement `HostFunction` for a function that has the same arity than the tuple.
#[allow(unused_parens)]
impl< $( $x, )* Rets, RetsAsResult, Func >
    crate::HostFunction<(), ( $( $x ),* ), Rets, WithoutEnv>
for
    Func
where
    $( $x: FromToNativeWasmType, )*
    Rets: WasmTypeList,
    RetsAsResult: IntoResult<Rets>,
    Func: Fn($( $x , )*) -> RetsAsResult + 'static
{


  #[cfg(feature = "jsc")]
  #[allow(non_snake_case)]
  fn jsc_function_callback(&self) -> crate::rt::jsc::vm::VMFunctionCallback {
    use crate::rt::jsc::utils::convert::AsJsc;
    use rusty_jsc::{JSObject, JSValue, callback};


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

          let func: &Func = &*(&() as *const () as *const Func);
          let global = ctx.get_global_object();
          let store_ptr = global.get_property(&ctx, "__store_ptr".to_string()).to_number(&ctx).unwrap();
          if store_ptr.is_nan() {
              panic!("Store pointer is invalid. Received {}", store_ptr as usize)
          }

          let mut store = StoreMut::from_raw(store_ptr as usize as *mut _);
          let result = panic::catch_unwind(AssertUnwindSafe(|| {
              type JSArray<'a> = &'a [JSValue; count_idents!( $( $x ),* )];
              let args_without_store: JSArray = arguments.try_into().unwrap();
              let [ $( $x ),* ] = args_without_store;
              func($( FromToNativeWasmType::from_native( $x::Native::from_raw(&mut store, RawValue { u128: {
                  // TODO: This may not be the fastest way, but JSC doesn't expose a BigInt interface
                  // so the only thing we can do is parse from the string repr
                  if $x.is_number(&ctx) {
                      $x.to_number(&ctx).unwrap() as _
                  }
                  else {
                      $x.to_string(&ctx).unwrap().to_string().parse::<u128>().unwrap()
                  }
              } }) ) ),* ).into_result()
          }));

          match result {
              Ok(Ok(result)) => {
                  match Rets::size() {
                      0 => {Ok(JSValue::undefined(&ctx))},
                      1 => {
                          let ty = Rets::wasm_types()[0];
                          let mut arr = result.into_array(&mut store);
                          let val = Value::from_raw(&mut store, ty, arr.as_mut()[0]);
                          let value: JSValue = val.as_jsc_value(&store);
                          Ok(value)
                      }
                      _n => {
                          let mut arr = result.into_array(&mut store);
                          let result_values = Rets::wasm_types().iter().enumerate().map(|(i, ret_type)| {
                              let raw = arr.as_mut()[i];
                              Value::from_raw(&mut store, *ret_type, raw).as_jsc_value(&mut store)
                          }).collect::<Vec<_>>();
                          Ok(JSObject::new_array(&ctx, &result_values).unwrap().to_jsvalue())
                      }
                  }
              },
              #[cfg(feature = "std")]
              Ok(Err(err)) => {
                  let trap = crate::rt::jsc::error::Trap::user(Box::new(err));
                  Err(trap.into_jsc_value(&ctx))
              },
              #[cfg(feature = "core")]
              Ok(Err(err)) => {
                  let trap = crate::rt::jsc::error::Trap::user(Box::new(err));
                  Err(trap.into_jsc_value(&ctx))
              },
              Err(panic) => {
                  Err(JSValue::string(&ctx, format!("panic: {:?}", panic)))
                  // We can't just resume the unwind, because it will put
                  // JavacriptCore in a bad state, so we need to transform
                  // the error

                  // std::panic::resume_unwind(panic)
              },
          }

      }
      Some(fn_callback::< $( $x, )* Rets, RetsAsResult, Self > as _)
  }

  #[cfg(feature = "js")]
  #[allow(non_snake_case)]
  fn js_function_callback(&self) -> crate::rt::js::vm::VMFunctionCallback {
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
              Ok(Err(trap)) => crate::rt::js::error::raise(Box::new(trap)),
              #[cfg(feature = "core")]
              #[allow(deprecated)]
              Ok(Err(trap)) => crate::rt::js::error::raise(Box::new(trap)),
              Err(_panic) => unimplemented!(),
          }
      }

      func_wrapper::< $( $x, )* Rets, RetsAsResult, Self > as _
  }

  #[cfg(feature = "wamr")]
  #[allow(non_snake_case)]
  fn wamr_function_callback(&self) -> crate::rt::wamr::vm::VMFunctionCallback {
      use crate::rt::wamr::bindings::*;
      use crate::rt::wamr::utils::convert::*;
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
          let mut r: *mut crate::rt::wamr::function::FunctionCallbackEnv<Func> = unsafe {std::mem::transmute(env)};
          let store = &mut (*r).store.as_store_mut();
          let mut i = 0;

          $(
              let c_arg = (*(*args).data.wrapping_add(i)).clone();
              let wasmer_arg = c_arg.into_wv();
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

                  let mut c_results: Vec<wasm_val_t> = native_results.into_iter().map(IntoCApiValue::into_cv).collect();

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
                  let trap =  crate::rt::wamr::error::Trap::user(Box::new(e));
                  unsafe { trap.into_wasm_trap(store) }
                  // unimplemented!("host function panicked");
              },

              Err(e) => {
                  unimplemented!("host function panicked");
              }
          }
      }
      func_wrapper::< $( $x, )* Rets, RetsAsResult, Self > as _
  }

  #[cfg(feature = "v8")]
  #[allow(non_snake_case)]
  fn v8_function_callback(&self) -> crate::rt::v8::vm::VMFunctionCallback {
      use crate::rt::v8::bindings::*;
      use crate::rt::v8::utils::convert::*;

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
  	func_wrapper::< $( $x, )* Rets, RetsAsResult, Self > as _
  }

  #[cfg(feature = "sys")]
  #[allow(non_snake_case)]
  fn sys_function_callback(&self) -> crate::rt::sys::vm::VMFunctionCallback {
	unsafe extern "C" fn func_wrapper<$( $x, )* Rets, RetsAsResult, Func>( env: &crate::rt::sys::function::StaticFunction<Func, ()>, $( $x: <$x::Native as NativeWasmType>::Abi, )* ) -> Rets::CStruct
    where
	  $( $x: FromToNativeWasmType, )*
	  Rets: WasmTypeList,
	  RetsAsResult: IntoResult<Rets>,
	  Func: Fn($( $x , )*) -> RetsAsResult + 'static,
    {
	  let mut store = StoreMut::from_raw(env.raw_store as *mut _);
	  let result = wasmer_vm::on_host_stack(|| {
	      panic::catch_unwind(AssertUnwindSafe(|| {
	          $( let $x = FromToNativeWasmType::from_native(NativeWasmTypeInto::from_abi(&mut store, $x));)*
	          (env.func)($($x),*).into_result()
	      }))
	  });

	  match result {
	      Ok(Ok(result)) => return result.into_c_struct(&mut store),
	      Ok(Err(trap)) => wasmer_vm::raise_user_trap(Box::new(trap)),
	      Err(panic) => wasmer_vm::resume_panic(panic) ,
	  }
    }
    func_wrapper::< $( $x, )* Rets, RetsAsResult, Self > as _
  }

  #[cfg(feature = "sys")]
  #[allow(non_snake_case)]
  fn sys_call_trampoline_address() -> crate::rt::sys::vm::VMTrampoline {
	unsafe extern "C" fn call_trampoline<$( $x: FromToNativeWasmType, )* Rets: WasmTypeList>(
          vmctx: *mut crate::rt::sys::vm::VMContext,
          body: crate::rt::sys::vm::VMFunctionCallback,
          args: *mut RawValue,
    ) {
	  let body: unsafe extern "C" fn(vmctx: *mut crate::rt::sys::vm::VMContext, $( $x: <$x::Native as NativeWasmType>::Abi, )*) -> Rets::CStruct = std::mem::transmute(body);
	  let mut _n = 0;
	  $(
	  let $x = *args.add(_n).cast();
	  _n += 1;
	  )*

	  let results = body(vmctx, $( $x ),*);
	  Rets::write_c_struct_to_ptr(results, args);
    }
	  call_trampoline::<$( $x, )* Rets> as _
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
