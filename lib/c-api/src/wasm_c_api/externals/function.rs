use super::super::store::{StoreRef, wasm_store_t};
use super::super::trap::wasm_trap_t;
use super::super::types::{wasm_functype_t, wasm_ref_t, wasm_valkind_enum};
use super::super::value::{wasm_val_inner, wasm_val_t, wasm_val_vec_t};
use super::wasm_extern_t;
use crate::error::update_last_error;
use crate::wasm_c_api::function_env::FunctionCEnv;
use libc::c_void;
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

/// Convert host-callback argument [`Value`]s into C `wasm_val_t`s, boxing
/// reference values into `store`.
fn callback_args_to_wasm(args: &[Value], store: &StoreRef) -> Result<wasm_val_vec_t, RuntimeError> {
    let vals = args
        .iter()
        .map(|v| wasm_val_t::from_value(v, store))
        .collect::<Result<Vec<wasm_val_t>, &'static str>>()
        .map_err(RuntimeError::new)?;
    Ok(vals.into())
}

/// Convert the C `wasm_val_t`s produced by a host callback back into [`Value`]s.
fn callback_results_to_values(results: Vec<wasm_val_t>) -> Result<Vec<Value>, RuntimeError> {
    results
        .into_iter()
        .map(Value::try_from)
        .collect::<Result<Vec<Value>, &'static str>>()
        .map_err(RuntimeError::new)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_new(
    store: Option<&mut wasm_store_t>,
    function_type: Option<&wasm_functype_t>,
    callback: Option<wasm_func_callback_t>,
) -> Option<Box<wasm_func_t>> {
    let function_type = function_type?;
    let callback = callback?;
    let store = store?;
    // Capture a weak store handle (not a strong `StoreRef`) to avoid a
    // store → function → store cycle; upgrade it per call to box ref args.
    let store_weak = store.inner.downgrade();
    let mut store_mut = unsafe { store.inner.store_mut() };

    let func_sig = &function_type.inner().function_type;
    let num_rets = func_sig.results().len();
    let inner_callback = move |mut _env: FunctionEnvMut<'_, FunctionCEnv>,
                               args: &[Value]|
          -> Result<Vec<Value>, RuntimeError> {
        let store = store_weak
            .upgrade()
            .ok_or_else(|| RuntimeError::new("store was dropped"))?;
        let processed_args = callback_args_to_wasm(args, &store)?;

        let mut results: wasm_val_vec_t = vec![
            wasm_val_t {
                kind: wasm_valkind_enum::WASM_I64 as _,
                of: wasm_val_inner { int64_t: 0 },
            };
            num_rets
        ]
        .into();

        let trap = unsafe { callback(&processed_args, &mut results) };

        if let Some(trap) = trap {
            return Err(trap.inner);
        }

        callback_results_to_values(results.take())
    };
    let env = FunctionEnv::new(&mut store_mut, FunctionCEnv::default());
    let function = Function::new_with_env(&mut store_mut, &env, func_sig, inner_callback);
    Some(Box::new(wasm_func_t {
        extern_: wasm_extern_t::new(store.inner.clone(), function.into()),
    }))
}

#[unsafe(no_mangle)]
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
    let store_weak = store.inner.downgrade();
    let mut store_mut = unsafe { store.inner.store_mut() };

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
            if let Ok(mut guard) = self.env_finalizer.lock()
                && Arc::strong_count(&self.env_finalizer) == 1
                && let Some(env_finalizer) = guard.take()
            {
                unsafe { (env_finalizer)(self.env.as_ptr()) };
            }
        }
    }
    let inner_callback = move |env: FunctionEnvMut<'_, WrapperEnv>,
                               args: &[Value]|
          -> Result<Vec<Value>, RuntimeError> {
        let store = store_weak
            .upgrade()
            .ok_or_else(|| RuntimeError::new("store was dropped"))?;
        let processed_args = callback_args_to_wasm(args, &store)?;

        let mut results: wasm_val_vec_t = vec![
            wasm_val_t {
                kind: wasm_valkind_enum::WASM_I64 as _,
                of: wasm_val_inner { int64_t: 0 },
            };
            num_rets
        ]
        .into();

        let trap = unsafe { callback(env.data().env.as_ptr(), &processed_args, &mut results) };

        if let Some(trap) = trap {
            return Err(trap.inner);
        }

        callback_results_to_values(results.take())
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

#[unsafe(no_mangle)]
pub extern "C" fn wasm_func_copy(func: &wasm_func_t) -> Box<wasm_func_t> {
    Box::new(func.clone())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_delete(_func: Option<Box<wasm_func_t>>) {}

// A `funcref` view of a function, and back. Per `wasm.h` these are non-owning
// views; since our `wasm_ref_t` is a distinct allocation the returned ref is
// typically never freed and leaks. It holds only a weak store handle, so it
// does not pin the store.
//
// NOTE: storing the resulting funcref into a table or global works only for
// *static* functions. Dynamic host functions (created via `wasm_func_new`) have
// no funcref representation in the sys VM and will abort on `table.set` — a
// separate VM limitation, not something this shim can work around.

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_as_ref(
    func: Option<&mut wasm_func_t>,
) -> Option<Box<wasm_ref_t>> {
    let func = func?;
    wasm_ref_t::new(
        func.extern_.store.clone(),
        Value::FuncRef(Some(func.extern_.function())),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_as_ref_const(
    func: Option<&wasm_func_t>,
) -> Option<Box<wasm_ref_t>> {
    let func = func?;
    wasm_ref_t::new(
        func.extern_.store.clone(),
        Value::FuncRef(Some(func.extern_.function())),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_as_func(
    ref_: Option<&mut wasm_ref_t>,
) -> Option<Box<wasm_func_t>> {
    let ref_ = ref_?;
    let func = match &ref_.inner {
        Value::FuncRef(Some(f)) => f.clone(),
        _ => return None,
    };
    let store = ref_.store.upgrade()?;
    Some(Box::new(wasm_func_t {
        extern_: wasm_extern_t::new(store, func.into()),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_as_func_const(
    ref_: Option<&wasm_ref_t>,
) -> Option<Box<wasm_func_t>> {
    let ref_ = ref_?;
    let func = match &ref_.inner {
        Value::FuncRef(Some(f)) => f.clone(),
        _ => return None,
    };
    let store = ref_.store.upgrade()?;
    Some(Box::new(wasm_func_t {
        extern_: wasm_extern_t::new(store, func.into()),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_call(
    func: Option<&mut wasm_func_t>,
    args: Option<&wasm_val_vec_t>,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>> {
    let func = func?;
    let args = args?;
    let store_ref = func.extern_.store.clone();
    let mut store = func.extern_.store.clone();
    let mut store_mut = unsafe { store.store_mut() };
    // Convert by reference (not `.cloned()`): a shallow clone of a ref-carrying
    // `wasm_val_t` would double-free the boxed `wasm_ref_t`.
    let params = c_try!(
        args.as_slice()
            .iter()
            .map(Value::try_from)
            .collect::<Result<Vec<Value>, _>>()
    );

    match func.extern_.function().call(&mut store_mut, &params) {
        Ok(wasm_results) => {
            for (slot, val) in results
                .as_uninit_slice()
                .iter_mut()
                .zip(wasm_results.iter())
            {
                let converted = c_try!(wasm_val_t::from_value(val, &store_ref));
                *slot = MaybeUninit::new(converted);
            }

            None
        }
        Err(e) => Some(Box::new(e.into())),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_param_arity(func: Option<&wasm_func_t>) -> usize {
    let Some(func) = func else {
        update_last_error("func pointer is null");
        return 0;
    };
    let store_ref = unsafe { func.extern_.store.store() };
    func.extern_.function().ty(&store_ref).params().len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_result_arity(func: Option<&wasm_func_t>) -> usize {
    let Some(func) = func else {
        update_last_error("func pointer is null");
        return 0;
    };
    let store_ref = unsafe { func.extern_.store.store() };
    func.extern_.function().ty(&store_ref).results().len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_func_type(
    func: Option<&wasm_func_t>,
) -> Option<Box<wasm_functype_t>> {
    let func = func?;
    let store_ref = unsafe { func.extern_.store.store() };
    Some(Box::new(wasm_functype_t::new(
        func.extern_.function().ty(&store_ref),
    )))
}
