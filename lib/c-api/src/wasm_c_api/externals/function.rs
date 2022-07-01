use super::super::context::{wasm_context_ref_mut_t, wasm_context_t};
use super::super::store::wasm_store_t;
use super::super::trap::wasm_trap_t;
use super::super::types::{wasm_functype_t, wasm_valkind_enum};
use super::super::value::{wasm_val_inner, wasm_val_t, wasm_val_vec_t};
use super::CApiExternTag;
use std::cell::RefCell;
use std::convert::TryInto;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::rc::Rc;
use wasmer_api::{Function, RuntimeError, Value};

#[derive(Debug, Clone)]
#[allow(non_camel_case_types)]
#[repr(C)]
pub struct wasm_func_t {
    pub(crate) tag: CApiExternTag,
    pub(crate) inner: Box<Function>,
    pub(crate) context: Option<Rc<RefCell<wasm_context_t>>>,
}

impl wasm_func_t {
    pub(crate) fn new(function: Function) -> Self {
        Self {
            tag: CApiExternTag::Function,
            inner: Box::new(function),
            context: None,
        }
    }
}

#[allow(non_camel_case_types)]
pub type wasm_func_callback_t = unsafe extern "C" fn(
    context: &mut wasm_context_ref_mut_t,
    args: &wasm_val_vec_t,
    results: &mut wasm_val_vec_t,
) -> Option<Box<wasm_trap_t>>;

#[no_mangle]
pub unsafe extern "C" fn wasm_func_new(
    store: Option<&wasm_store_t>,
    function_type: Option<&wasm_functype_t>,
    callback: Option<wasm_func_callback_t>,
) -> Option<Box<wasm_func_t>> {
    let function_type = function_type?;
    let callback = callback?;
    let store = store?;
    if store.context.is_none() {
        crate::error::update_last_error(wasm_store_t::CTX_ERR_STR);
    }
    let mut ctx = store.context.as_ref()?.borrow_mut();

    let func_sig = &function_type.inner().function_type;
    let num_rets = func_sig.results().len();
    let inner_callback = move |ctx: wasmer_api::ContextMut<'_, *mut c_void>,
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

        let trap = callback(
            &mut wasm_context_ref_mut_t { inner: ctx },
            &processed_args,
            &mut results,
        );

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
    let function = Function::new(&mut ctx.inner, func_sig, inner_callback);
    drop(ctx);
    let mut retval = Box::new(wasm_func_t::new(function));
    retval.context = store.context.clone();

    Some(retval)
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
    if func.context.is_none() {
        crate::error::update_last_error(wasm_store_t::CTX_ERR_STR);
    }
    let mut ctx = func.context.as_ref()?.borrow_mut();

    let params = args
        .as_slice()
        .iter()
        .cloned()
        .map(TryInto::try_into)
        .collect::<Result<Vec<Value>, _>>()
        .expect("Arguments conversion failed");

    match func.inner.call(&mut ctx.inner, &params) {
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
    let ctx = func
        .context
        .as_ref()
        .expect(wasm_store_t::CTX_ERR_STR)
        .borrow();
    func.inner.ty(&ctx.inner).params().len()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_func_result_arity(func: &wasm_func_t) -> usize {
    let ctx = func
        .context
        .as_ref()
        .expect(wasm_store_t::CTX_ERR_STR)
        .borrow();
    func.inner.ty(&ctx.inner).results().len()
}

#[no_mangle]
pub extern "C" fn wasm_func_type(func: Option<&wasm_func_t>) -> Option<Box<wasm_functype_t>> {
    let func = func?;
    if func.context.is_none() {
        crate::error::update_last_error(wasm_store_t::CTX_ERR_STR);
    }
    let ctx = func.context.as_ref()?.borrow();

    Some(Box::new(wasm_functype_t::new(func.inner.ty(&ctx.inner))))
}
