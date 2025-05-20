//! Native Functions.
//!
//! This module creates the helper `TypedFunction` that let us call WebAssembly
//! functions with the native ABI, that is:
//!
//! ```ignore
//! let add_one = instance.exports.get_function("function_name")?;
//! let add_one_native: TypedFunction<i32, i32> = add_one.typed().unwrap();
//! ```

use std::iter::FromIterator;

use crate::{
    backend::wamr::{bindings::*, error::Trap, function::Function, utils::convert::*},
    AsStoreMut, FromToNativeWasmType, NativeWasmType, NativeWasmTypeInto, RuntimeError,
    TypedFunction, Value, WasmTypeList,
};
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
            pub fn call_wamr(&self, mut store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType, )*
            {

                 // // let store_ptr = Value::I64(store.as_store_mut().as_raw() as _).as_jsvalue(store);
                 #[allow(unused_unsafe)]
                 let params_list: Vec<_> = unsafe {
                     vec![ $( {
                         let raw = $x.to_native().into_raw(store);
                         let value = Value::from_raw(&mut store, <$x::Native as NativeWasmType>::WASM_TYPE, raw);
                         value.into_cv()
                     } ),* ]
                 };

                 let mut results = {
                    unsafe {
                        let mut ret = std::mem::zeroed();
                        wasm_val_vec_new_uninitialized(&mut ret, Rets::wasm_types().len());
                        ret
                    }
                };


                 let func = unsafe { wasm_extern_as_func(self.func.to_vm_extern().into_wamr()) };

                let mut params = unsafe {std::mem::zeroed()};
                unsafe {wasm_val_vec_new(&mut params, params_list.len(), params_list.as_ptr())};

                 let mut trap;

                 loop {
                    trap = unsafe { wasm_func_call(func, &params, &mut results) };

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
                };



                 if !trap.is_null() {
                     unsafe {
                        let trap: Trap = trap.into();
                        return Err(RuntimeError::from(trap));
                     }
                }

                unsafe {
                    let mut rets_list_array = Rets::empty_array();
                    let mut_rets = rets_list_array.as_mut() as *mut [RawValue] as *mut RawValue;

                    match Rets::size() {
                        0 => {},
                        1 => {
                            let val = (*results.data.wrapping_add(0)).clone();
                            let val = val.into_wv();
                            *mut_rets = val.as_raw(&mut store);
                        }
                        _n => {
                            for (i, ret_type) in Rets::wasm_types().iter().enumerate() {
                                    let val = (*results.data.wrapping_add(i)).clone();
                                    let val = val.into_wv();
                                    let slot = mut_rets.add(i);
                                    *slot = val.as_raw(&mut store);
                            }
                        }
                    }

                    Ok(unsafe { Rets::from_array(store, rets_list_array) })
                }
            }

            #[doc(hidden)]
            #[allow(missing_docs)]
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call_raw_wamr(&self, store: &mut impl AsStoreMut, mut params_list: Vec<RawValue> ) -> Result<Rets, RuntimeError> {
                todo!("Raw calls from wamr are not supported yet!")
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
