//! Native Functions.
//!
//! This module creates the helper `TypedFunction` that let us call WebAssembly
//! functions with the native ABI, that is:
//!
//! ```ignore
//! let add_one = instance.exports.get_function("function_name")?;
//! let add_one_native: TypedFunction<i32, i32> = add_one.typed().unwrap();
//! ```
use crate::{
    AsStoreMut, FromToNativeWasmType, NativeWasmType, NativeWasmTypeInto, RuntimeError,
    TypedFunction, Value, WasmTypeList,
    js::utils::convert::{AsJs, js_value_to_wasmer},
};
#[cfg(feature = "experimental-async")]
use crate::{AsStoreAsync, StoreAsync};
use js_sys::Array;
#[cfg(feature = "experimental-async")]
use std::future::Future;
use std::iter::FromIterator;
use wasm_bindgen::JsValue;
use wasmer_types::RawValue;

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        #[allow(unused_parens, non_snake_case)]
        impl<$( $x , )* Rets> TypedFunction<( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func asynchronously through JSPI.
            #[allow(clippy::too_many_arguments)]
            #[cfg(feature = "experimental-async")]
            pub(crate) fn call_async_js(
                func: crate::Function,
                store: StoreAsync,
                $( $x: $x, )*
            ) -> impl Future<Output = Result<Rets, RuntimeError>> + 'static
            where
                $( $x: FromToNativeWasmType + 'static, )*
            {
                async move {
                    let mut write = store.write_lock().await;
                    let func_ty = func.ty(&mut write);
                    let mut params_raw = [ $( $x.to_native().into_raw(&mut write) ),* ];
                    let mut params_values = Vec::with_capacity(params_raw.len());
                    for (raw, ty) in params_raw.iter().zip(func_ty.params()) {
                        unsafe {
                            params_values.push(Value::from_raw(&mut write, *ty, *raw));
                        }
                    }
                    drop(write);

                    let results = func.call_async(&store, params_values).await?;
                    let mut write = store.write_lock().await;
                    convert_results::<Rets>(&mut write, func_ty, &results)
                }
            }

            /// Call the typed func and return results.
            #[allow(clippy::too_many_arguments)]
            pub fn call_js(&self, mut store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType,  )*
            {
                #[allow(unused_unsafe)]
                let params_list: Vec<_> = unsafe {
                    vec![ $( (<$x::Native as NativeWasmType>::WASM_TYPE, $x.to_native().into_raw(store) ) ),* ]
                };
                let results = {
                    let mut r;
                    // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
                    loop {
                        let args_array = unsafe {
                            Array::from_iter(
                                params_list
                                    .clone()
                                    .into_iter()
                                    .map(|(b, a)| Value::from_raw(store, b, a).as_jsvalue(store)),
                            )
                        };
                        r = self
                            .func
                            .as_js()
                            .handle
                            .function
                            .apply(&JsValue::UNDEFINED, &args_array);
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
                let mut_rets = rets_list_array.as_mut() as *mut [RawValue] as *mut RawValue;
                match Rets::size() {
                    0 => {},
                    1 => unsafe {
                        let ty = Rets::wasm_types()[0];
                        let val = js_value_to_wasmer(&mut store, &ty, &results);
                        *mut_rets = val.as_raw(&mut store);
                    }
                    _n => {
                        let results: Array = results.into();
                        for (i, ret_type) in Rets::wasm_types().iter().enumerate() {
                            let ret = results.get(i as u32);
                            unsafe {
                                let val = js_value_to_wasmer(&mut store, &ret_type, &ret);
                                let slot = mut_rets.add(i);
                                *slot = val.as_raw(&mut store);
                            }
                        }
                    }
                }
                Ok(unsafe { Rets::from_array(store, rets_list_array) })
            }

            #[doc(hidden)]
            #[allow(missing_docs)]
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call_raw_js(&self, store: &mut impl AsStoreMut, mut params_list: Vec<RawValue> ) -> Result<Rets, RuntimeError> {
                todo!("Raw calls from js are not supported yet!")
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
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15
);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16
);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17
);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18
);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19
);
impl_native_traits!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20
);

#[cfg(feature = "experimental-async")]
fn convert_results<Rets: WasmTypeList>(
    store: &mut impl AsStoreMut,
    func_ty: wasmer_types::FunctionType,
    results: &[Value],
) -> Result<Rets, RuntimeError> {
    if results.len() != func_ty.results().len() {
        return Err(RuntimeError::new("result arity mismatch"));
    }

    let mut rets_list_array = Rets::empty_array();
    for ((slot, ty), value) in rets_list_array
        .as_mut()
        .iter_mut()
        .zip(func_ty.results())
        .zip(results)
    {
        debug_assert_eq!(value.ty(), *ty);
        *slot = value.as_raw(store);
    }

    Ok(unsafe { Rets::from_array(store, rets_list_array) })
}
