use super::{wasm_externtype_t, wasm_valtype_t, wasm_valtype_vec_t, WasmExternType};
use std::mem;
use std::ptr::NonNull;
use wasmer::{ExternType, FunctionType, ValType};

#[derive(Debug)]
pub(crate) struct WasmFunctionType {
    pub(crate) function_type: FunctionType,
    params: Box<wasm_valtype_vec_t>,
    results: Box<wasm_valtype_vec_t>,
}

impl WasmFunctionType {
    pub(crate) fn new(function_type: FunctionType) -> Self {
        let params = {
            let mut valtypes = function_type
                .params()
                .iter()
                .cloned()
                .map(Into::into)
                .map(Box::new)
                .map(Box::into_raw)
                .collect::<Vec<*mut wasm_valtype_t>>();

            let valtypes_vec = Box::new(wasm_valtype_vec_t {
                size: valtypes.len(),
                data: valtypes.as_mut_ptr(),
            });

            mem::forget(valtypes);

            valtypes_vec
        };
        let results = {
            let mut valtypes = function_type
                .results()
                .iter()
                .cloned()
                .map(Into::into)
                .map(Box::new)
                .map(Box::into_raw)
                .collect::<Vec<*mut wasm_valtype_t>>();

            let valtypes_vec = Box::new(wasm_valtype_vec_t {
                size: valtypes.len(),
                data: valtypes.as_mut_ptr(),
            });

            mem::forget(valtypes);

            valtypes_vec
        };

        Self {
            function_type,
            params,
            results,
        }
    }
}

impl Clone for WasmFunctionType {
    fn clone(&self) -> Self {
        Self::new(self.function_type.clone())
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(transparent)]
pub struct wasm_functype_t {
    pub(crate) extern_type: wasm_externtype_t,
}

impl wasm_functype_t {
    pub(crate) fn new(function_type: FunctionType) -> Self {
        Self {
            extern_type: wasm_externtype_t::new(ExternType::Function(function_type)),
        }
    }

    pub(crate) fn inner(&self) -> &WasmFunctionType {
        match &self.extern_type.inner {
            WasmExternType::Function(wasm_function_type) => &wasm_function_type,
            _ => {
                unreachable!("Data corruption: `wasm_functype_t` does not contain a function type")
            }
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
    let params = params?;
    let results = results?;

    let params: Vec<ValType> = params
        .as_ref()
        .into_slice()?
        .iter()
        .map(|ptr| **ptr)
        .map(Into::into)
        .collect::<Vec<_>>();
    let results: Vec<ValType> = results
        .as_ref()
        .into_slice()?
        .iter()
        .map(|ptr| **ptr)
        .map(Into::into)
        .collect::<Vec<_>>();

    Some(Box::new(wasm_functype_t::new(FunctionType::new(
        params, results,
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_delete(_function_type: Option<Box<wasm_functype_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_copy(
    function_type: Option<NonNull<wasm_functype_t>>,
) -> Option<Box<wasm_functype_t>> {
    let function_type = function_type?;

    Some(Box::new(wasm_functype_t::new(
        function_type.as_ref().inner().function_type.clone(),
    )))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_params(
    function_type: &wasm_functype_t,
) -> *const wasm_valtype_vec_t {
    function_type.inner().params.as_ref() as *const _
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_results(
    function_type: &wasm_functype_t,
) -> *const wasm_valtype_vec_t {
    function_type.inner().results.as_ref() as *const _
}
