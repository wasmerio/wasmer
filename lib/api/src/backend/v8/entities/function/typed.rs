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
    AsStoreMut, CallWrapper, FromToNativeWasmType, NativeWasmType, NativeWasmTypeInto,
    RuntimeError, TypedFunction, Value, WasmTypeList,
    backend::v8::{bindings::*, error::Trap, function::Function, utils::convert::*},
    entities::function::call_wrapper::call_wrapped,
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
            #[cfg(feature = "experimental-async")]
            #[allow(clippy::too_many_arguments, unused_mut)]
            pub(crate) fn call_async_v8(
                func: crate::Function,
                store: StoreAsync,
                $( $x: $x, )*
            ) -> impl Future<Output = Result<Rets, RuntimeError>> + 'static
            where
                $( $x: FromToNativeWasmType + 'static, )*
            {
                async move {
                    let mut write = store.write_lock().await;
                    if !func.is_from_store(&write) { return Err(RuntimeError::new("function belongs to another store")); }
                    let func_ty = func.ty(&write);
                    let params_raw = [ $( $x.to_native().into_raw(&mut write) ),* ];
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
            pub fn call_v8(&self, store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType, )*
            {
                self.call_wrapped_v8(store, None, $( $x, )*)
            }

            /// Call the typed func through `wrapper` and return results.
            #[allow(clippy::too_many_arguments)]
            pub(crate) fn call_wrapped_v8(&self, mut store: &mut impl AsStoreMut, wrapper: Option<&mut dyn CallWrapper>, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where
            $( $x: FromToNativeWasmType, )*
            {

                 // // let store_ptr = Value::I64(store.as_store_mut().as_raw() as _).as_jsvalue(store);
                 #[allow(unused_unsafe)]
                 let params_list: Vec<_> = unsafe {
                     vec![ $( {
                         let raw = $x.to_native().into_raw(store);
                         let value = Value::from_raw(&mut *store, <$x::Native as NativeWasmType>::WASM_TYPE, raw);
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


                 let func = unsafe { wasm_extern_as_func(self.func.to_vm_extern().unwrap_v_8()) };

                 call_wrapped(store, wrapper, |store| {
                    // `wasm_func_call` takes the arguments, so every entry gets its own.
                    let mut params = unsafe { std::mem::zeroed() };
                    let trap = unsafe {
                        wasm_val_vec_new(&mut params, params_list.len(), params_list.as_ptr());
                        crate::v8::enter_sync(&mut store.as_store_mut(), || wasm_func_call(func, &params, &mut results))
                    };
                    if trap.is_null() {
                        Ok(())
                    } else {
                        Err(RuntimeError::from(Trap::from(trap)))
                    }
                 })?;

                unsafe {
                    let mut rets_list_array = Rets::empty_array();
                    let mut_rets = rets_list_array.as_mut() as *mut [RawValue] as *mut RawValue;

                    for (i, ty) in Rets::wasm_types().iter().enumerate() {
                        let mut value = *results.data.add(i);
                        value.kind = ty.into_ct();
                        *mut_rets.add(i) = value.into_wv(self.func.as_v8().store_id).as_raw(store);
                    }

                    Ok(unsafe { Rets::from_array(store, rets_list_array) })
                }
            }

            #[doc(hidden)]
            #[allow(missing_docs)]
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call_raw_v8(&self, store: &mut impl AsStoreMut, mut params_list: Vec<RawValue> ) -> Result<Rets, RuntimeError> {
                todo!("Raw calls from v8 are not supported yet!")
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
use crate::{AsStoreAsync, StoreAsync};
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
