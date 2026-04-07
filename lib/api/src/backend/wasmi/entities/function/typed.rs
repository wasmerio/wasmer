//! Native Functions.

#![allow(missing_docs)]

use crate::{
    AsStoreMut, FromToNativeWasmType, NativeWasmType, NativeWasmTypeInto, RuntimeError,
    TypedFunction, Value, WasmTypeList,
    backend::wasmi::{function::Function, utils::convert::{IntoCApiType, IntoCApiValue, IntoWasmerValue}},
};
use ::wasmi;
use wasmer_types::RawValue;

macro_rules! impl_native_traits {
    ( $( $x:ident ),* ) => {
        #[allow(unused_parens, non_snake_case)]
        impl<$( $x, )* Rets> TypedFunction<( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            #[allow(clippy::too_many_arguments)]
            pub fn call_wasmi(&self, mut store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                let params_list: Vec<wasmi::Val> = unsafe {
                    vec![ $({
                        let raw = $x.to_native().into_raw(&mut store);
                        let value = Value::from_raw(&mut store, <$x::Native as NativeWasmType>::WASM_TYPE, raw);
                        value.into_cv()
                    }),* ]
                };

                let result_types = Rets::wasm_types();
                let mut results: Vec<wasmi::Val> = result_types
                    .iter()
                    .copied()
                    .map(|ty| wasmi::Val::default(ty.into_ct()))
                    .collect();

                self.func
                    .as_wasmi()
                    .handle
                    .call(&mut store.as_store_mut().inner.store.as_wasmi_mut().inner, &params_list, &mut results)
                    .map_err(crate::backend::wasmi::error::Trap::from_wasmi_error)?;

                unsafe {
                    let mut rets_list_array = Rets::empty_array();
                    let mut_rets = rets_list_array.as_mut() as *mut [RawValue] as *mut RawValue;

                    match Rets::size() {
                        0 => {}
                        1 => {
                            let val = results.remove(0).into_wv();
                            *mut_rets = val.as_raw(&mut store);
                        }
                        _ => {
                            for (i, val) in results.into_iter().enumerate() {
                                let slot = mut_rets.add(i);
                                *slot = val.into_wv().as_raw(&mut store);
                            }
                        }
                    }

                    Ok(Rets::from_array(store, rets_list_array))
                }
            }

            pub fn call_raw_wasmi(&self, _store: &mut impl AsStoreMut, _params_list: Vec<RawValue>) -> Result<Rets, RuntimeError> {
                todo!("Raw calls from wasmi are not supported yet!")
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
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19);
impl_native_traits!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20);
