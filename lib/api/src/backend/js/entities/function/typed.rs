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
    AsStoreMut, CallWrapper, FromToNativeWasmType, NativeWasmType, NativeWasmTypeInto,
    RuntimeError, TypedFunction, Value, WasmTypeList,
    entities::function::call_wrapper::call_wrapped,
    js::{
        function::numeric,
        utils::convert::{AsJs, js_value_to_wasmer},
    },
};
#[cfg(feature = "experimental-async")]
use crate::{AsStoreAsync, StoreAsync};
use js_sys::Array;
#[cfg(feature = "experimental-async")]
use std::future::Future;
use std::iter::FromIterator;
use wasm_bindgen::JsValue;
use wasmer_types::{RawValue, Type};

/// The raw arguments of a call as the numbers JavaScript knows them by.
/// `None` if there are too many for the numeric lane, or if one is of a type
/// whose values are not numbers there.
fn raw_numbers(params: &[(Type, RawValue)]) -> Option<numeric::Numbers> {
    let mut numbers = [0.0; numeric::MAX_ARITY];
    if params.len() > numbers.len() {
        return None;
    }
    for (number, (ty, raw)) in numbers.iter_mut().zip(params) {
        *number = match ty {
            Type::I32 => unsafe { raw.i32 as f64 },
            Type::F32 => unsafe { raw.f32 as f64 },
            Type::F64 => unsafe { raw.f64 },
            _ => return None,
        };
    }
    Some((numbers, params.len()))
}

/// The type of the one result of a function on the numeric lane, if it has
/// one. `None` for results that keep a function off the lane.
fn numeric_result(results: &[Type]) -> Option<Option<Type>> {
    match results {
        [] => Some(None),
        [ty @ (Type::I32 | Type::F32 | Type::F64)] => Some(Some(*ty)),
        _ => None,
    }
}

/// The number the engine returned for a result of type `ty`, as a raw value.
fn number_to_raw(ty: Type, number: f64) -> RawValue {
    match ty {
        Type::I32 => RawValue { i32: number as i32 },
        Type::F32 => RawValue { f32: number as f32 },
        _ => RawValue { f64: number },
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
            pub fn call_js(&self, store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType,  )*
            {
                self.call_wrapped_js(store, None, $( $x, )*)
            }

            /// Call the typed func through `wrapper` and return results.
            #[allow(clippy::too_many_arguments)]
            pub(crate) fn call_wrapped_js(&self, mut store: &mut impl AsStoreMut, wrapper: Option<&mut dyn CallWrapper>, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType,  )*
            {
                #[allow(unused_unsafe)]
                let params_list: [(Type, RawValue); _] = unsafe {
                    [ $( (<$x::Native as NativeWasmType>::WASM_TYPE, $x.to_native().into_raw(store) ) ),* ]
                };
                // Nothing but numbers: they are passed as they are, without
                // an array of arguments in between.
                if let (Some(numbers), Some(result_ty)) =
                    (raw_numbers(&params_list), numeric_result(Rets::wasm_types()))
                {
                    // Called with the receiver of the call below, and its
                    // result read the same way.
                    let function = &self.func.as_js().handle.function;
                    let mut result = 0.0;
                    call_wrapped(store, wrapper, |_| {
                        let _entered = super::enter_sync();
                        let receiver = &JsValue::UNDEFINED;
                        result = numeric::call(function, receiver, &numbers, result_ty.is_some())?;
                        Ok(())
                    })?;
                    let mut rets_list_array = Rets::empty_array();
                    if let Some(ty) = result_ty {
                        rets_list_array.as_mut()[0] = number_to_raw(ty, result);
                    }
                    return Ok(unsafe { Rets::from_array(store, rets_list_array) });
                }
                let mut results = JsValue::UNDEFINED;
                call_wrapped(store, wrapper, |store| {
                    let args_array = unsafe {
                        Array::from_iter(
                            params_list
                                .iter()
                                .map(|&(b, a)| Value::from_raw(store, b, a).as_jsvalue(store)),
                        )
                    };
                    let _entered = super::enter_sync();
                    results = self
                        .func
                        .as_js()
                        .handle
                        .function
                        .apply(&JsValue::UNDEFINED, &args_array)?;
                    Ok(())
                })?;
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
