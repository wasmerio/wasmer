use super::super::store::wasm_store_t;
use super::super::trap::wasm_trap_t;
use super::super::types::{wasm_functype_t, wasm_valkind_enum};
use super::super::value::{wasm_val_inner, wasm_val_t, wasm_val_vec_t};
use super::wasm_extern_t;
use crate::wasm_c_api::function_env::FunctionCEnv;
use libc::c_void;
use std::convert::TryInto;
use std::mem::MaybeUninit;
use std::sync::{Arc, Mutex};
use wasmer_api::{Extern, Function, FunctionEnv, FunctionEnvMut, RuntimeError, Value};

#[derive(Clone)]
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct wasm_func_t {
    pub(crate) extern_: wasm_extern_t,
}

impl wasm_func_t {
    pub(crate) fn try_from(e: &wasm_extern_t) -> Option<&wasm_func_t> {
        match &e.inner {
            Extern::Function(_) => Some(unsafe { &*(e as *const _ as *const _) }),
            _ => None,
        }
    }
}

#[allow(non_camel_case_types)]
pub type wasm_func_callback_t = unsafe extern "C" fn(
    args: &wasm_val_vec_t,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>>;

#[allow(non_camel_case_types)]
pub type wasm_func_callback_with_env_t = unsafe extern "C" fn(
    env: *mut c_void,
    args: &wasm_val_vec_t,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>>;

#[allow(non_camel_case_types)]
pub type wasm_env_finalizer_t = unsafe extern "C" fn(*mut c_void);

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new(
    store: Option<&mut wasm_store_t>,
    function_type: Option<&wasm_functype_t>,
    callback: Option<wasm_func_callback_t>,
) -> Option<Box<wasm_func_t>> {
    let function_type = function_type?;
    let callback = callback?;
    let store = store?;
    let mut store_mut = store.inner.store_mut();

    let func_sig = &function_type.inner().function_type;
    let num_rets = func_sig.results().len();
    let inner_callback = move |mut _env: FunctionEnvMut<'_, FunctionCEnv>,
                               args: &[Value]|
          -> Result<Vec<Value>, RuntimeError> {
        let processed_args: wasm_val_vec_t = args
            .iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<wasm_val_t>, _>>()
            .expect("Argument conversion failed")
            .into();

        let mut results: wasm_val_vec_t = vec![
            wasm_val_t {
                kind: wasm_valkind_enum::WASM_I64 as _,
                of: wasm_val_inner { int64_t: 0 },
            };
            num_rets
        ]
        .into();

        let trap = callback(&processed_args, &mut results);

        if let Some(trap) = trap {
            return Err(trap.inner);
        }

        let processed_results = results
            .take()
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Value>, _>>()
            .expect("Result conversion failed");

        Ok(processed_results)
    };
    let env = FunctionEnv::new(&mut store_mut, FunctionCEnv::default());
    let function = Function::new_with_env(&mut store_mut, &env, func_sig, inner_callback);
    Some(Box::new(wasm_func_t {
        extern_: wasm_extern_t::new(store.inner.clone(), function.into()),
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_with_env(
    store: Option<&mut wasm_store_t>,
    function_type: Option<&wasm_functype_t>,
    callback: Option<wasm_func_callback_with_env_t>,
    env: *mut c_void,
    env_finalizer: Option<wasm_env_finalizer_t>,
) -> Option<Box<wasm_func_t>> {
    let function_type = function_type?;
    let callback = callback?;
    let store = store?;
    let mut store_mut = store.inner.store_mut();

    let func_sig = &function_type.inner().function_type;
    let num_rets = func_sig.results().len();

    #[derive(Clone)]
    #[repr(C)]
    struct WrapperEnv {
        env: FunctionCEnv,
        env_finalizer: Arc<Mutex<Option<wasm_env_finalizer_t>>>,
    }

    // Only relevant when using multiple threads in the C API;
    // Synchronization will be done via the C API / on the C side.
    unsafe impl Send for WrapperEnv {}
    unsafe impl Sync for WrapperEnv {}

    impl Drop for WrapperEnv {
        fn drop(&mut self) {
            if let Ok(mut guard) = self.env_finalizer.lock() {
                if Arc::strong_count(&self.env_finalizer) == 1 {
                    if let Some(env_finalizer) = guard.take() {
                        unsafe { (env_finalizer)(self.env.as_ptr()) };
                    }
                }
            }
        }
    }
    let inner_callback = move |env: FunctionEnvMut<'_, WrapperEnv>,
                               args: &[Value]|
          -> Result<Vec<Value>, RuntimeError> {
        let processed_args: wasm_val_vec_t = args
            .iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<wasm_val_t>, _>>()
            .expect("Argument conversion failed")
            .into();

        let mut results: wasm_val_vec_t = vec![
            wasm_val_t {
                kind: wasm_valkind_enum::WASM_I64 as _,
                of: wasm_val_inner { int64_t: 0 },
            };
            num_rets
        ]
        .into();

        let trap = callback(env.data().env.as_ptr(), &processed_args, &mut results);

        if let Some(trap) = trap {
            return Err(trap.inner);
        }

        let processed_results = results
            .take()
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Value>, _>>()
            .expect("Result conversion failed");

        Ok(processed_results)
    };
    let env = FunctionEnv::new(
        &mut store_mut,
        WrapperEnv {
            env: FunctionCEnv::new(c_try!(
                std::ptr::NonNull::new(env),
                "Function environment cannot be a null pointer."
            )),
            env_finalizer: Arc::new(Mutex::new(env_finalizer)),
        },
    );
    let function = Function::new_with_env(&mut store_mut, &env, func_sig, inner_callback);
    Some(Box::new(wasm_func_t {
        extern_: wasm_extern_t::new(store.inner.clone(), function.into()),
    }))
}

#[no_mangle]
pub extern "C" fn wasm_func_copy(func: &wasm_func_t) -> Box<wasm_func_t> {
    Box::new(func.clone())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_delete(_func: Option<Box<wasm_func_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_call(
    func: Option<&mut wasm_func_t>,
    args: Option<&wasm_val_vec_t>,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>> {
    let func = func?;
    let args = args?;
    let mut store = func.extern_.store.clone();
    let mut store_mut = store.store_mut();
    let params = args
        .as_slice()
        .iter()
        .cloned()
        .map(TryInto::try_into)
        .collect::<Result<Vec<Value>, _>>()
        .expect("Arguments conversion failed");

    match func.extern_.function().call(&mut store_mut, &params) {
        Ok(wasm_results) => {
            for (slot, val) in results
                .as_uninit_slice()
                .iter_mut()
                .zip(wasm_results.iter())
            {
                *slot = MaybeUninit::new(val.try_into().expect("Results conversion failed"));
            }

            None
        }
        Err(e) => Some(Box::new(e.into())),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_param_arity(func: &wasm_func_t) -> usize {
    func.extern_
        .function()
        .ty(&func.extern_.store.store())
        .params()
        .len()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_result_arity(func: &wasm_func_t) -> usize {
    func.extern_
        .function()
        .ty(&func.extern_.store.store())
        .results()
        .len()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_type(
    func: Option<&wasm_func_t>,
) -> Option<Box<wasm_functype_t>> {
    let func = func?;
    Some(Box::new(wasm_functype_t::new(
        func.extern_.function().ty(&func.extern_.store.store()),
    )))
}
