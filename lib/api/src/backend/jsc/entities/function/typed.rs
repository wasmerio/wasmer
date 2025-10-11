//! Native Functions.
//!
//! This module creates the helper `TypedFunction` that let us call WebAssembly
//! functions with the native ABI, that is:
//!
//! ```ignore
//! let add_one = instance.exports.get_function("function_name")?;
//! let add_one_native: TypedFunction<i32, i32> = add_one.typed().unwrap();
//! ```
// use std::panic::{catch_unwind, AssertUnwindSafe};
use crate::{
    AsStoreMut, FromToNativeWasmType, NativeWasmType, NativeWasmTypeInto, RuntimeError,
    TypedFunction, Value, WasmTypeList,
    backend::jsc::{
        engine::IntoJSC,
        error::Trap,
        utils::convert::{AsJsc, jsc_value_to_wasmer},
    },
};

use rusty_jsc::JSValue;
use std::iter::FromIterator;
use wasmer_types::RawValue;

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
            pub fn call_jsc(&self, mut store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType, )*
            {
                #[allow(unused_unsafe)]
                let params_list: Vec<_> = unsafe {
                    vec![ $( {
                        let raw = $x.to_native().into_raw(store);
                        let value = Value::from_raw(&mut store, <$x::Native as NativeWasmType>::WASM_TYPE, raw);
                        value.as_jsc_value(store)
                    } ),* ]
                };

                let store_mut = store.as_store_mut();
                let context = store_mut.jsc().context();

                let global = context.get_global_object();
                let store_ptr = store_mut.as_raw() as usize;
                global.set_property(context, "__store_ptr".to_string(), JSValue::number(context, store_ptr as _)).unwrap();

                let results = {
                    let mut r;
                    // TODO: This loop is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
                    loop {

                        let store_mut = store.as_store_mut();
                        let context = store_mut.jsc().context();
                        r = self.func.as_jsc().handle.function
                            .call(
                                context,
                                None,
                                &params_list,
                            );

                        // let store_mut = store.as_store_mut();
                        if let Some(callback) = store_mut.inner.on_called.take() {
                            match callback(store_mut) {
                                Ok(wasmer_types::OnCalledAction::InvokeAgain) => { continue; }
                                Ok(wasmer_types::OnCalledAction::Finish) => { break; }
                                Ok(wasmer_types::OnCalledAction::Trap(trap)) => { let err = Err(RuntimeError::user(trap));
                                    return err;
                                },
                                Err(trap) => {
                                    return Err(RuntimeError::user(trap))
                                },
                            }
                        }
                        break;
                    }
                    let store_mut = store.as_store_mut();
                    let context = store_mut.jsc().context();
                    r.map_err(|e| Trap::from_jsc_value(context, e))?
                };
                let mut rets_list_array = Rets::empty_array();
                let mut_rets = rets_list_array.as_mut() as *mut [RawValue] as *mut RawValue;
                let store_mut = store.as_store_mut();
                let context = store_mut.jsc().context();
                match Rets::size() {
                    0 => {},
                    1 => unsafe {
                        let ty = Rets::wasm_types()[0];
                        let val = jsc_value_to_wasmer(context, &ty, &results);
                        *mut_rets = val.as_raw(&mut store);
                    }
                    _n => {
                        if !results.is_array(context) {
                            panic!("Expected results to be an array.")
                        }
                        let results = results.to_object(context).unwrap();
                        for (i, ret_type) in Rets::wasm_types().iter().enumerate() {
                            let store_mut = store.as_store_mut();
                            let context = store_mut.jsc().context();
                            let ret = results.get_property_at_index(context, i as _).unwrap();
                            unsafe {
                                let val = jsc_value_to_wasmer(context, &ret_type, &ret);
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
            pub fn call_raw_jsc(&self, store: &mut impl AsStoreMut, mut params_list: Vec<RawValue> ) -> Result<Rets, RuntimeError> {
                todo!("Raw calls from jsc are not supported yet!")
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
