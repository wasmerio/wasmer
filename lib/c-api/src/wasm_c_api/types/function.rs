use super::{wasm_externtype_t, wasm_valtype_vec_t, WasmExternType};
use std::fmt;
use wasmer_api::{ExternType, FunctionType};
use wasmer_types::Type;

pub(crate) struct WasmFunctionType {
    pub(crate) function_type: FunctionType,
    params: wasm_valtype_vec_t,
    results: wasm_valtype_vec_t,
}

impl WasmFunctionType {
    pub(crate) fn new(function_type: FunctionType) -> Self {
        let params: Vec<_> = function_type
            .params()
            .iter()
            .map(|&valtype| Some(Box::new(valtype.into())))
            .collect();
        let results: Vec<_> = function_type
            .results()
            .iter()
            .map(|&valtype| Some(Box::new(valtype.into())))
            .collect();

        Self {
            function_type,
            params: params.into(),
            results: results.into(),
        }
    }
}

impl Clone for WasmFunctionType {
    fn clone(&self) -> Self {
        Self::new(self.function_type.clone())
    }
}

impl fmt::Debug for WasmFunctionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.function_type.fmt(f)
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
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
            WasmExternType::Function(wasm_function_type) => wasm_function_type,
            _ => {
                unreachable!("Data corruption: `wasm_functype_t` does not contain a function type")
            }
        }
    }
}

wasm_declare_boxed_vec!(functype);
wasm_impl_copy_delete!(functype);

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_new(
    params: Option<&mut wasm_valtype_vec_t>,
    results: Option<&mut wasm_valtype_vec_t>,
) -> Option<Box<wasm_functype_t>> {
    let params = params?;
    let results = results?;

    let params_as_valtype: Vec<Type> = params
        .take()
        .into_iter()
        .map(|val| val.as_ref().unwrap().as_ref().into())
        .collect::<Vec<_>>();
    let results_as_valtype: Vec<Type> = results
        .take()
        .into_iter()
        .map(|val| val.as_ref().unwrap().as_ref().into())
        .collect::<Vec<_>>();

    Some(Box::new(wasm_functype_t::new(FunctionType::new(
        params_as_valtype,
        results_as_valtype,
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_params(
    function_type: Option<&wasm_functype_t>,
) -> Option<&wasm_valtype_vec_t> {
    let function_type = function_type?;

    Some(&function_type.inner().params)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_results(
    function_type: Option<&wasm_functype_t>,
) -> Option<&wasm_valtype_vec_t> {
    let function_type = function_type?;

    Some(&function_type.inner().results)
}
