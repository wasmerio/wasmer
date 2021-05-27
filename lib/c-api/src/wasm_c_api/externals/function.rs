use super::super::store::wasm_store_t;
use super::super::trap::wasm_trap_t;
use super::super::types::{wasm_functype_t, wasm_valkind_enum};
use super::super::value::{wasm_val_inner, wasm_val_t, wasm_val_vec_t};
use super::CApiExternTag;
use std::convert::TryInto;
use std::ffi::c_void;
use std::sync::Arc;
use wasmer::{Function, RuntimeError, Val};

#[derive(Debug, Clone)]
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct wasm_func_t {
    pub(crate) tag: CApiExternTag,
    pub(crate) inner: Box<Function>,
}

impl wasm_func_t {
    pub(crate) fn new(function: Function) -> Self {
        Self {
            tag: CApiExternTag::Function,
            inner: Box::new(function),
        }
    }
}

#[allow(non_camel_case_types)]
pub type wasm_func_callback_t = unsafe extern "C" fn(
    args: *const wasm_val_vec_t,
    results: *mut wasm_val_vec_t,
) -> *mut wasm_trap_t;

#[allow(non_camel_case_types)]
pub type wasm_func_callback_with_env_t = unsafe extern "C" fn(
    env: *mut c_void,
    args: *const wasm_val_vec_t,
    results: *mut wasm_val_vec_t,
) -> *mut wasm_trap_t;

#[allow(non_camel_case_types)]
pub type wasm_env_finalizer_t = unsafe extern "C" fn(*mut c_void);

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new(
    store: Option<&wasm_store_t>,
    function_type: Option<&wasm_functype_t>,
    callback: Option<wasm_func_callback_t>,
) -> Option<Box<wasm_func_t>> {
    let store = store?;
    let function_type = function_type?;
    let callback = callback?;

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
    let function = Function::new(&store.inner, func_sig, inner_callback);

    Some(Box::new(wasm_func_t::new(function)))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new_with_env(
    store: Option<&wasm_store_t>,
    function_type: Option<&wasm_functype_t>,
    callback: Option<wasm_func_callback_with_env_t>,
    env: *mut c_void,
    env_finalizer: Option<wasm_env_finalizer_t>,
) -> Option<Box<wasm_func_t>> {
    let store = store?;
    let function_type = function_type?;
    let callback = callback?;

    let func_sig = &function_type.inner().function_type;
    let num_rets = func_sig.results().len();

    #[derive(wasmer::WasmerEnv, Clone)]
    #[repr(C)]
    struct WrapperEnv {
        env: *mut c_void,
        env_finalizer: Arc<Option<wasm_env_finalizer_t>>,
    }

    // Only relevant when using multiple threads in the C API;
    // Synchronization will be done via the C API / on the C side.
    unsafe impl Send for WrapperEnv {}
    unsafe impl Sync for WrapperEnv {}

    impl Drop for WrapperEnv {
        fn drop(&mut self) {
            if let Some(env_finalizer) =
                Arc::get_mut(&mut self.env_finalizer).and_then(Option::take)
            {
                if !self.env.is_null() {
                    unsafe { (env_finalizer)(self.env as _) }
                }
            }
        }
    }

    let trampoline = move |env: &WrapperEnv, args: &[Val]| -> Result<Vec<Val>, RuntimeError> {
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

        let trap = callback(env.env, &processed_args, &mut results);

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

    let function = Function::new_with_env(
        &store.inner,
        func_sig,
        WrapperEnv {
            env,
            env_finalizer: Arc::new(env_finalizer),
        },
        trampoline,
    );

    Some(Box::new(wasm_func_t::new(function)))
}

#[no_mangle]
pub extern "C" fn wasm_func_copy(func: &wasm_func_t) -> Box<wasm_func_t> {
    Box::new(func.clone())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_delete(_func: Option<Box<wasm_func_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_call(
    func: Option<&wasm_func_t>,
    args: Option<&wasm_val_vec_t>,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>> {
    let func = func?;
    let args = args?;

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
pub extern "C" fn wasm_func_type(func: Option<&wasm_func_t>) -> Option<Box<wasm_functype_t>> {
    let func = func?;

    Some(Box::new(wasm_functype_t::new(func.inner.ty().clone())))
}
