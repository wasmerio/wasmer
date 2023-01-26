//! Native Functions.
//!
//! This module creates the helper `TypedFunction` that let us call WebAssembly
//! functions with the native ABI, that is:
//!
//! ```ignore
//! let add_one = instance.exports.get_function("function_name")?;
//! let add_one_native: TypedFunction<i32, i32> = add_one.typed().unwrap();
//! ```
use std::marker::PhantomData;

use crate::js::externals::Function;
use crate::js::store::{AsStoreMut, AsStoreRef};
use crate::js::{FromToNativeWasmType, RuntimeError, WasmTypeList};
// use std::panic::{catch_unwind, AssertUnwindSafe};
use crate::js::types::param_from_js;
use crate::js::types::AsJs;
use crate::js::vm::VMFunction;
use js_sys::Array;
use std::iter::FromIterator;
use wasm_bindgen::JsValue;
use wasmer_types::RawValue;

/// A WebAssembly function that can be called natively
/// (using the Native ABI).
#[derive(Clone)]
pub struct TypedFunction<Args = (), Rets = ()> {
    pub(crate) handle: VMFunction,
    _phantom: PhantomData<(Args, Rets)>,
}

unsafe impl<Args, Rets> Send for TypedFunction<Args, Rets> {}
unsafe impl<Args, Rets> Sync for TypedFunction<Args, Rets> {}

impl<Args, Rets> TypedFunction<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    #[allow(dead_code)]
    pub(crate) fn new<T>(_store: &mut impl AsStoreMut, vm_function: VMFunction) -> Self {
        Self {
            handle: vm_function,
            _phantom: PhantomData,
        }
    }

    pub(crate) fn from_handle(f: Function) -> Self {
        Self {
            handle: f.handle,
            _phantom: PhantomData,
        }
    }
}

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        #[allow(unused_parens, non_snake_case)]
        impl<$( $x , )* Rets> TypedFunction<( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            #[allow(clippy::too_many_arguments)]
            pub fn call(&self, mut store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType + crate::js::NativeWasmTypeInto, )*
            {
                #[allow(unused_unsafe)]
                let params_list: Vec<RawValue> = unsafe {
                    vec![ $( RawValue { f64: $x.into_raw(store) } ),* ]
                };
                let params_list: Vec<JsValue> = params_list
                    .into_iter()
                    .map(|a| a.as_jsvalue(&store.as_store_ref()))
                    .collect();
                let results = {
                    let mut r;
                    // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
                    loop {
                        r = self.handle.function.apply(
                            &JsValue::UNDEFINED,
                            &Array::from_iter(params_list.iter())
                        );
                        let store_mut = store.as_store_mut();
                        if let Some(callback) = store_mut.inner.on_called.take() {
                            match callback(store_mut) {
                                Ok(wasmer_types::OnCalledAction::InvokeAgain) => { continue; }
                                Ok(wasmer_types::OnCalledAction::Finish) => { break; }
                                Ok(wasmer_types::OnCalledAction::Trap(trap)) => { return Err(RuntimeError::user(trap)) },
                                Err(trap) => { return Err(RuntimeError::user(trap)) },
                            }
                        }
                        break;
                    }
                    r?
                };
                let mut rets_list_array = Rets::empty_array();
                let mut_rets = rets_list_array.as_mut() as *mut [f64] as *mut f64;
                match Rets::size() {
                    0 => {},
                    1 => unsafe {
                        let ty = Rets::wasm_types()[0];
                        let val = param_from_js(&ty, &results);
                        *mut_rets = val.as_raw(&mut store);
                    }
                    _n => {
                        let results: Array = results.into();
                        for (i, ret_type) in Rets::wasm_types().iter().enumerate() {
                            let ret = results.get(i as u32);
                            unsafe {
                                let val = param_from_js(&ret_type, &ret);
                                let slot = mut_rets.add(i);
                                *slot = val.as_raw(&mut store);
                            }
                        }
                    }
                }
                Ok(unsafe { Rets::from_array(store, rets_list_array) })
            }
        }

        #[allow(unused_parens)]
        impl<'a, $( $x, )* Rets> crate::js::exports::ExportableWithGenerics<'a, ($( $x ),*), Rets> for TypedFunction<( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            fn get_self_from_extern_with_generics(store: &impl AsStoreRef, _extern: &crate::js::externals::Extern) -> Result<Self, crate::js::exports::ExportError> {
                use crate::js::exports::Exportable;
                crate::js::Function::get_self_from_extern(_extern)?.typed(store).map_err(|_| crate::js::exports::ExportError::IncompatibleType)
            }
        }
    };
}

impl_native_traits!();
impl_native_traits!(A1);
impl_native_traits!(A1, A2);
impl_native_traits!(A1, A2, A3);
impl_native_traits!(A1, A2, A3, A4);
impl_native_traits!(A1, A2, A3, A4, A5);
impl_native_traits!(A1, A2, A3, A4, A5, A6);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18
);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19
);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20
);
