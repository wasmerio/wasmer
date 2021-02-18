use super::{wasm_externtype_t, wasm_valtype_vec_delete, wasm_valtype_vec_t, WasmExternType};
use wasmer::{ExternType, FunctionType, ValType};

#[derive(Debug)]
pub(crate) struct WasmFunctionType {
    pub(crate) function_type: FunctionType,
    params: Box<wasm_valtype_vec_t>,
    results: Box<wasm_valtype_vec_t>,
}

impl WasmFunctionType {
    pub(crate) fn new(function_type: FunctionType) -> Self {
        let params: Box<wasm_valtype_vec_t> = Box::new(function_type.params().into());
        let results: Box<wasm_valtype_vec_t> = Box::new(function_type.results().into());

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
            WasmExternType::Function(wasm_function_type) => &wasm_function_type,
            _ => {
                unreachable!("Data corruption: `wasm_functype_t` does not contain a function type")
            }
        }
    }
}

wasm_declare_boxed_vec!(functype);

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_new(
    params: Option<&mut wasm_valtype_vec_t>,
    results: Option<&mut wasm_valtype_vec_t>,
) -> Option<Box<wasm_functype_t>> {
    let params = params?;
    let results = results?;

    let params_as_valtype: Vec<ValType> = params
        .into_slice()?
        .into_iter()
        .map(|val| val.as_ref().into())
        .collect::<Vec<_>>();
    let results_as_valtype: Vec<ValType> = results
        .into_slice()?
        .iter()
        .map(|val| val.as_ref().into())
        .collect::<Vec<_>>();

    wasm_valtype_vec_delete(Some(params));
    wasm_valtype_vec_delete(Some(results));

    Some(Box::new(wasm_functype_t::new(FunctionType::new(
        params_as_valtype,
        results_as_valtype,
    ))))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_delete(_function_type: Option<Box<wasm_functype_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_copy(
    function_type: Option<&wasm_functype_t>,
) -> Option<Box<wasm_functype_t>> {
    let function_type = function_type?;

    Some(Box::new(function_type.clone()))
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_params(
    function_type: Option<&wasm_functype_t>,
) -> Option<&wasm_valtype_vec_t> {
    let function_type = function_type?;

    Some(function_type.inner().params.as_ref())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_functype_results(
    function_type: Option<&wasm_functype_t>,
) -> Option<&wasm_valtype_vec_t> {
    let function_type = function_type?;

    Some(function_type.inner().results.as_ref())
}
