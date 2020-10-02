use super::{wasm_externtype_t, wasm_valtype_t, wasm_valtype_vec_t};
use std::mem;
use std::ptr::NonNull;
use wasmer::{ExternType, FunctionType, ValType};

#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub struct wasm_functype_t {
    pub(crate) extern_: wasm_externtype_t,
}

impl wasm_functype_t {
    pub(crate) fn sig(&self) -> &FunctionType {
        if let ExternType::Function(ref f) = self.extern_.inner {
            f
        } else {
            unreachable!("data corruption: `wasm_functype_t` does not contain a function")
        }
    }
}

wasm_declare_vec!(functype);

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_new(
    // own
    params: Option<NonNull<wasm_valtype_vec_t>>,
    // own
    results: Option<NonNull<wasm_valtype_vec_t>>,
) -> Option<Box<wasm_functype_t>> {
    wasm_functype_new_inner(params?, results?)
}

unsafe fn wasm_functype_new_inner(
    // own
    params: NonNull<wasm_valtype_vec_t>,
    // own
    results: NonNull<wasm_valtype_vec_t>,
) -> Option<Box<wasm_functype_t>> {
    let params = params.as_ref();
    let results = results.as_ref();
    let params: Vec<ValType> = params
        .into_slice()?
        .iter()
        .map(|&ptr| *ptr)
        .map(Into::into)
        .collect::<Vec<_>>();
    let results: Vec<ValType> = results
        .into_slice()?
        .iter()
        .map(|&ptr| *ptr)
        .map(Into::into)
        .collect::<Vec<_>>();

    let extern_ = wasm_externtype_t {
        inner: ExternType::Function(FunctionType::new(params, results)),
    };
    Some(Box::new(wasm_functype_t { extern_ }))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_delete(_ft: Option<Box<wasm_functype_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_copy(
    arg: Option<NonNull<wasm_functype_t>>,
) -> Option<Box<wasm_functype_t>> {
    let arg = arg?;
    let funcsig = arg.as_ref();
    Some(Box::new(funcsig.clone()))
}

// TODO: fix memory leak
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_params(ft: &wasm_functype_t) -> *const wasm_valtype_vec_t {
    let mut valtypes = ft
        .sig()
        .params()
        .iter()
        .cloned()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_valtype_t>>();
    let out = Box::into_raw(Box::new(wasm_valtype_vec_t {
        size: valtypes.len(),
        data: valtypes.as_mut_ptr(),
    }));
    mem::forget(valtypes);
    out as *const _
}

// TODO: fix memory leak
#[no_mangle]
pub unsafe extern "C" fn wasm_functype_results(ft: &wasm_functype_t) -> *const wasm_valtype_vec_t {
    let mut valtypes = ft
        .sig()
        .results()
        .iter()
        .cloned()
        .map(Into::into)
        .map(Box::new)
        .map(Box::into_raw)
        .collect::<Vec<*mut wasm_valtype_t>>();
    let out = Box::into_raw(Box::new(wasm_valtype_vec_t {
        size: valtypes.len(),
        data: valtypes.as_mut_ptr(),
    }));
    mem::forget(valtypes);
    out as *const _
}
