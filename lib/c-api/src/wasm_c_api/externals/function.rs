use super::super::store::wasm_store_t;
use super::super::trap::wasm_trap_t;
use super::super::types::{wasm_functype_t, wasm_valkind_enum};
use super::super::value::{wasm_val_inner, wasm_val_t};
use std::convert::TryInto;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use wasmer::{Function, Instance, RuntimeError, Store, Val};

/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub struct wasm_func_t {
    pub(crate) inner: Function,
    // this is how we ensure the instance stays alive
    pub(crate) instance: Option<Arc<Instance>>,
}

/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub type wasm_func_callback_t =
    unsafe extern "C" fn(args: *const wasm_val_t, results: *mut wasm_val_t) -> *mut wasm_trap_t;

/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub type wasm_func_callback_with_env_t = unsafe extern "C" fn(
    *mut c_void,
    args: *const wasm_val_t,
    results: *mut wasm_val_t,
) -> *mut wasm_trap_t;

/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub type wasm_env_finalizer_t = unsafe extern "C" fn(c_void);

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_func_new(
    store: Option<NonNull<wasm_store_t>>,
    ft: &wasm_functype_t,
    callback: wasm_func_callback_t,
) -> Option<Box<wasm_func_t>> {
    // TODO: handle null pointers?
    let store_ptr = store?.cast::<Store>();
    let store = store_ptr.as_ref();
    let func_sig = ft.sig();
    let num_rets = func_sig.results().len();
    let inner_callback = move |args: &[Val]| -> Result<Vec<Val>, RuntimeError> {
        let processed_args = args
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<wasm_val_t>, _>>()
            .expect("Argument conversion failed");

        let mut results = vec![
            wasm_val_t {
                kind: wasm_valkind_enum::WASM_I64 as _,
                of: wasm_val_inner { int64_t: 0 },
            };
            num_rets
        ];

        let trap = callback(processed_args.as_ptr(), results.as_mut_ptr());
        if !trap.is_null() {
            let trap: Box<wasm_trap_t> = Box::from_raw(trap);
            RuntimeError::raise(Box::new(trap.inner));
        }

        let processed_results = results
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Val>, _>>()
            .expect("Result conversion failed");
        Ok(processed_results)
    };
    let f = Function::new(store, &func_sig, inner_callback);
    Some(Box::new(wasm_func_t {
        instance: None,
        inner: f,
    }))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_with_env(
    store: Option<NonNull<wasm_store_t>>,
    ft: &wasm_functype_t,
    callback: wasm_func_callback_with_env_t,
    env: *mut c_void,
    finalizer: wasm_env_finalizer_t,
) -> Option<Box<wasm_func_t>> {
    // TODO: handle null pointers?
    let store_ptr = store?.cast::<Store>();
    let store = store_ptr.as_ref();
    let func_sig = ft.sig();
    let num_rets = func_sig.results().len();
    let inner_callback =
        move |env: &mut *mut c_void, args: &[Val]| -> Result<Vec<Val>, RuntimeError> {
            let processed_args = args
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<wasm_val_t>, _>>()
                .expect("Argument conversion failed");

            let mut results = vec![
                wasm_val_t {
                    kind: wasm_valkind_enum::WASM_I64 as _,
                    of: wasm_val_inner { int64_t: 0 },
                };
                num_rets
            ];

            let _traps = callback(*env, processed_args.as_ptr(), results.as_mut_ptr());
            // TODO: do something with `traps`

            let processed_results = results
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<Val>, _>>()
                .expect("Result conversion failed");
            Ok(processed_results)
        };
    let f = Function::new_with_env(store, &func_sig, env, inner_callback);
    Some(Box::new(wasm_func_t {
        instance: None,
        inner: f,
    }))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_func_delete(_func: Option<Box<wasm_func_t>>) {}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_func_call(
    func: &wasm_func_t,
    args: *const wasm_val_t,
    results: *mut wasm_val_t,
) -> Option<Box<wasm_trap_t>> {
    let num_params = func.inner.ty().params().len();
    let params: Vec<Val> = (0..num_params)
        .map(|i| (&(*args.add(i))).try_into())
        .collect::<Result<_, _>>()
        .ok()?;

    match func.inner.call(&params) {
        Ok(wasm_results) => {
            for (i, actual_result) in wasm_results.iter().enumerate() {
                let result_loc = &mut (*results.add(i));
                *result_loc = (&*actual_result).try_into().ok()?;
            }
            None
        }
        Err(e) => Some(Box::new(e.into())),
    }
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_func_param_arity(func: &wasm_func_t) -> usize {
    func.inner.ty().params().len()
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_func_result_arity(func: &wasm_func_t) -> usize {
    func.inner.ty().results().len()
}
