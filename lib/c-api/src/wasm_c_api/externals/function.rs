use super::super::store::wasm_store_t;
use super::super::trap::wasm_trap_t;
use super::super::types::{wasm_functype_t, wasm_valkind_enum};
use super::super::value::{wasm_val_inner, wasm_val_t, wasm_val_vec_t};
use std::convert::TryInto;
use std::ffi::c_void;
use std::sync::Arc;
use wasmer::{Function, Instance, RuntimeError, Val};

#[allow(non_camel_case_types)]
pub struct wasm_func_t {
    pub(crate) inner: Function,
    // this is how we ensure the instance stays alive
    pub(crate) instance: Option<Arc<Instance>>,
}

#[allow(non_camel_case_types)]
pub type wasm_func_callback_t = unsafe extern "C" fn(
    args: *const wasm_val_vec_t,
    results: *mut wasm_val_vec_t,
) -> *mut wasm_trap_t;

#[allow(non_camel_case_types)]
pub type wasm_func_callback_with_env_t = unsafe extern "C" fn(
    *mut c_void,
    args: *const wasm_val_vec_t,
    results: *mut wasm_val_vec_t,
) -> *mut wasm_trap_t;

#[allow(non_camel_case_types)]
pub type wasm_env_finalizer_t = unsafe extern "C" fn(c_void);

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new(
    store: &wasm_store_t,
    function_type: &wasm_functype_t,
    callback: wasm_func_callback_t,
) -> Option<Box<wasm_func_t>> {
    // TODO: handle null pointers?
    let func_sig = &function_type.inner().function_type;
    let num_rets = func_sig.results().len();
    let inner_callback = move |args: &[Val]| -> Result<Vec<Val>, RuntimeError> {
        let processed_args: wasm_val_vec_t = args
            .into_iter()
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

        if !trap.is_null() {
            let trap: Box<wasm_trap_t> = Box::from_raw(trap);

            return Err(trap.inner);
        }

        let processed_results = results
            .into_slice()
            .expect("Failed to convert `results` into a slice")
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Val>, _>>()
            .expect("Result conversion failed");

        Ok(processed_results)
    };
    let function = Function::new(&store.inner, &func_sig, inner_callback);

    Some(Box::new(wasm_func_t {
        instance: None,
        inner: function,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_with_env(
    store: &wasm_store_t,
    function_type: &wasm_functype_t,
    callback: wasm_func_callback_with_env_t,
    env: *mut c_void,
    finalizer: wasm_env_finalizer_t,
) -> Option<Box<wasm_func_t>> {
    // TODO: handle null pointers?
    let func_sig = &function_type.inner().function_type;
    let num_rets = func_sig.results().len();
    let inner_callback =
        move |env: &mut *mut c_void, args: &[Val]| -> Result<Vec<Val>, RuntimeError> {
            let processed_args: wasm_val_vec_t = args
                .into_iter()
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

            let trap = callback(*env, &processed_args, &mut results);

            if !trap.is_null() {
                let trap: Box<wasm_trap_t> = Box::from_raw(trap);

                return Err(trap.inner);
            }

            let processed_results = results
                .into_slice()
                .expect("Failed to convert `results` into a slice")
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<Val>, _>>()
                .expect("Result conversion failed");

            Ok(processed_results)
        };

    let function = Function::new_with_env(&store.inner, &func_sig, env, inner_callback);

    Some(Box::new(wasm_func_t {
        instance: None,
        inner: function,
    }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_delete(_func: Option<Box<wasm_func_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_call(
    func: &wasm_func_t,
    args: &wasm_val_vec_t,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>> {
    let params = args
        .into_slice()
        .map(|slice| {
            slice
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<Val>, _>>()
                .expect("Arguments conversion failed")
        })
        .unwrap_or_default();

    match func.inner.call(&params) {
        Ok(wasm_results) => {
            let vals = wasm_results
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<wasm_val_t>, _>>()
                .expect("Results conversion failed");

            // `results` is an uninitialized vector. Set a new value.
            if results.is_uninitialized() {
                *results = vals.into();
            }
            // `results` is an initialized but empty vector. Fill it
            // item per item.
            else {
                let slice = results
                    .into_slice_mut()
                    .expect("`wasm_func_call`, results' size is greater than 0 but data is NULL");

                for (result, value) in slice.iter_mut().zip(vals.iter()) {
                    (*result).kind = value.kind;
                    (*result).of = value.of;
                }
            }

            None
        }
        Err(e) => Some(Box::new(e.into())),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_param_arity(func: &wasm_func_t) -> usize {
    func.inner.ty().params().len()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_result_arity(func: &wasm_func_t) -> usize {
    func.inner.ty().results().len()
}

#[no_mangle]
pub extern "C" fn wasm_func_type(func: &wasm_func_t) -> Box<wasm_functype_t> {
    Box::new(wasm_functype_t::new(func.inner.ty().clone()))
}
