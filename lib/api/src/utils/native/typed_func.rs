//! Native Functions.
//!
//! This module creates the helper [`TypedFunction`] that let us call WebAssembly
//! functions with the native ABI, that is:
//!
//! ```ignore
//! let add_one = instance.exports.get_function("function_name")?;
//! let add_one_native: TypedFunction<i32, i32> = add_one.native().unwrap();
//! ```
use crate::{
    store::AsStoreRef, AsStoreMut, BackendStore, FromToNativeWasmType, Function,
    NativeWasmTypeInto, RuntimeError, WasmTypeList,
};
use std::marker::PhantomData;
use wasmer_types::RawValue;

/// A WebAssembly function that can be called natively
/// (using the Native ABI).
#[derive(Clone, Debug)]
pub struct TypedFunction<Args, Rets> {
    pub(crate) func: Function,
    _phantom: PhantomData<fn(Args) -> Rets>,
}

unsafe impl<Args, Rets> Send for TypedFunction<Args, Rets> {}
unsafe impl<Args, Rets> Sync for TypedFunction<Args, Rets> {}

impl<Args, Rets> TypedFunction<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    #[allow(dead_code)]
    pub(crate) fn new(_store: &impl AsStoreRef, func: Function) -> Self {
        Self {
            func,
            _phantom: PhantomData,
        }
    }

    pub(crate) fn into_function(self) -> Function {
        self.func
    }
}

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        paste::paste!{
        #[allow(unused_parens, non_snake_case)]
        impl<$( $x , )* Rets> TypedFunction<( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call(&self, store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> where $( $x: FromToNativeWasmType, )*

            {
                $(
                    let [<p_ $x>] = $x;
                )*
                match store.as_store_mut().inner.store {
                    #[cfg(feature = "sys")]
                    BackendStore::Sys(_) => self.call_sys(store, $([<p_ $x>]),*),
                    #[cfg(feature = "wamr")]
                    BackendStore::Wamr(_) => self.call_wamr(store, $([<p_ $x>]),*),
                    #[cfg(feature = "wasmi")]
                    BackendStore::Wasmi(_) => self.call_wasmi(store, $([<p_ $x>]),*),
                    #[cfg(feature = "v8")]
                    BackendStore::V8(_) => self.call_v8(store, $([<p_ $x>]),*),
                    #[cfg(feature = "js")]
                    BackendStore::Js(_) => self.call_js(store, $([<p_ $x>]),*),
                    #[cfg(feature = "jsc")]
                    BackendStore::Jsc(_) => self.call_jsc(store, $([<p_ $x>]),*),

                }
            }

            #[doc(hidden)]
            #[allow(missing_docs)]
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call_raw(&self, store: &mut impl AsStoreMut, mut params_list: Vec<RawValue> ) -> Result<Rets, RuntimeError> {
                match store.as_store_mut().inner.store {
                    #[cfg(feature = "sys")]
                    BackendStore::Sys(_) => self.call_raw_sys(store, params_list),
                    #[cfg(feature = "wamr")]
                    BackendStore::Wamr(_) => self.call_raw_wamr(store, params_list),
                    #[cfg(feature = "wasmi")]
                    BackendStore::Wasmi(_) => self.call_raw_wasmi(store, params_list),
                    #[cfg(feature = "v8")]
                    BackendStore::V8(_) => self.call_raw_v8(store, params_list),
                    #[cfg(feature = "js")]
                    BackendStore::Js(_) => self.call_raw_js(store, params_list),
                    #[cfg(feature = "jsc")]
                    BackendStore::Jsc(_) => self.call_raw_jsc(store, params_list),
                }
            }
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
