//! Native Functions.
//!
//! This module creates the helper `TypedFunction` that let us call WebAssembly
//! functions with the native ABI, that is:
//!
//! ```ignore
//! let add_one = instance.exports.get_function("function_name")?;
//! let add_one_native: TypedFunction<i32, i32> = add_one.native().unwrap();
//! ```
use std::marker::PhantomData;

use crate::sys::{
    AsStoreMut, FromToNativeWasmType, Function, NativeWasmTypeInto, RuntimeError, WasmTypeList,
};
use wasmer_types::RawValue;

/// A WebAssembly function that can be called natively
/// (using the Native ABI).
pub struct TypedFunction<Args, Rets> {
    pub(crate) func: Function,
    _phantom: PhantomData<fn(Args) -> Rets>,
}

unsafe impl<Args, Rets> Send for TypedFunction<Args, Rets> {}

impl<Args, Rets> TypedFunction<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    pub(crate) fn new(func: Function) -> Self {
        Self {
            func,
            _phantom: PhantomData,
        }
    }
}

impl<Args: WasmTypeList, Rets: WasmTypeList> Clone for TypedFunction<Args, Rets> {
    fn clone(&self) -> Self {
        Self {
            func: self.func.clone(),
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
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call(&self, store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                let anyfunc = unsafe {
                    *self.func
                        .handle
                        .get(store.as_store_ref().objects())
                        .anyfunc
                        .as_ptr()
                        .as_ref()
                };
                // Ensure all parameters come from the same context.
                if $(!FromToNativeWasmType::is_from_store(&$x, store) ||)* false {
                    return Err(RuntimeError::new(
                        "cross-`Context` values are not supported",
                    ));
                }
                // TODO: when `const fn` related features mature more, we can declare a single array
                // of the correct size here.
                let mut params_list = [ $( $x.to_native().into_raw(store) ),* ];
                let mut rets_list_array = Rets::empty_array();
                let rets_list: &mut [RawValue] = rets_list_array.as_mut();
                let using_rets_array;
                let args_rets: &mut [RawValue] = if params_list.len() > rets_list.len() {
                    using_rets_array = false;
                    params_list.as_mut()
                } else {
                    using_rets_array = true;
                    for (i, &arg) in params_list.iter().enumerate() {
                        rets_list[i] = arg;
                    }
                    rets_list.as_mut()
                };
                unsafe {
                    wasmer_vm::wasmer_call_trampoline(
                        store.as_store_ref().signal_handler(),
                        anyfunc.vmctx,
                        anyfunc.call_trampoline,
                        anyfunc.func_ptr,
                        args_rets.as_mut_ptr() as *mut u8,
                    )
                }?;
                let num_rets = rets_list.len();
                if !using_rets_array && num_rets > 0 {
                    let src_pointer = params_list.as_ptr();
                    let rets_list = &mut rets_list_array.as_mut()[0] as *mut RawValue;
                    unsafe {
                        // TODO: we can probably remove this copy by doing some clever `transmute`s.
                        // we know it's not overlapping because `using_rets_array` is false
                        std::ptr::copy_nonoverlapping(src_pointer,
                                                        rets_list,
                                                        num_rets);
                    }
                }
                Ok(unsafe { Rets::from_array(store, rets_list_array) })
                // TODO: When the Host ABI and Wasm ABI are the same, we could do this instead:
                // but we can't currently detect whether that's safe.
                //
                // let results = unsafe {
                //     wasmer_vm::catch_traps_with_result(self.vmctx, || {
                //         let f = std::mem::transmute::<_, unsafe extern "C" fn( *mut VMContext, $( $x, )*) -> Rets::CStruct>(self.address());
                //         // We always pass the vmctx
                //         f( self.vmctx, $( $x, )* )
                //     }).map_err(RuntimeError::from_trap)?
                // };
                // Ok(Rets::from_c_struct(results))
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
