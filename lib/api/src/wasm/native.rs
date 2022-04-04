//! Native Functions.
//!
//! This module creates the helper `NativeFunc` that let us call WebAssembly
//! functions with the native ABI, that is:
//!
//! ```ignore
//! let add_one = instance.exports.get_function("function_name")?;
//! let add_one_native: NativeFunc<i32, i32> = add_one.native().unwrap();
//! ```
use std::marker::PhantomData;

use crate::wasm::{FromToNativeWasmType, Function, RuntimeError, Store, WasmTypeList};
// use std::panic::{catch_unwind, AssertUnwindSafe};
use crate::wasm::export::VMFunction;

/// A WebAssembly function that can be called natively
/// (using the Native ABI).
#[derive(Clone)]
pub struct NativeFunc<Args = (), Rets = ()> {
    store: Store,
    exported: VMFunction,
    _phantom: PhantomData<(Args, Rets)>,
}

unsafe impl<Args, Rets> Send for NativeFunc<Args, Rets> {}

impl<Args, Rets> NativeFunc<Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    /*pub(crate) fn new(store: Store, exported: VMFunction) -> Self {
        Self {
            store,
            exported,
            _phantom: PhantomData,
        }
    }*/
}

impl<Args, Rets> From<&NativeFunc<Args, Rets>> for VMFunction
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: &NativeFunc<Args, Rets>) -> Self {
        other.exported.clone()
    }
}

impl<Args, Rets> From<NativeFunc<Args, Rets>> for Function
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: NativeFunc<Args, Rets>) -> Self {
        Self {
            store: other.store,
            exported: other.exported,
        }
    }
}

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        #[allow(unused_parens, non_snake_case)]
        impl<$( $x , )* Rets> NativeFunc<( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                panic!("Not implemented!")
            }

        }

        #[allow(unused_parens)]
        impl<'a, $( $x, )* Rets> crate::wasm::exports::ExportableWithGenerics<'a, ($( $x ),*), Rets> for NativeFunc<( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            fn get_self_from_extern_with_generics(_extern: &crate::wasm::externals::Extern) -> Result<Self, crate::wasm::exports::ExportError> {
                use crate::wasm::exports::Exportable;
                crate::wasm::Function::get_self_from_extern(_extern)?.native().map_err(|_| crate::wasm::exports::ExportError::IncompatibleType)
            }
        }
    };
}

impl_native_traits!();
impl_native_traits!(_A1);
impl_native_traits!(_A1, _A2);
impl_native_traits!(_A1, _A2, _A3);
impl_native_traits!(_A1, _A2, _A3, _A4);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13);
impl_native_traits!(_A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13, _A14);
impl_native_traits!(
    _A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13, _A14, _A15
);
impl_native_traits!(
    _A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13, _A14, _A15, _A16
);
impl_native_traits!(
    _A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13, _A14, _A15, _A16, _A17
);
impl_native_traits!(
    _A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13, _A14, _A15, _A16, _A17,
    _A18
);
impl_native_traits!(
    _A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13, _A14, _A15, _A16, _A17,
    _A18, _A19
);
impl_native_traits!(
    _A1, _A2, _A3, _A4, _A5, _A6, _A7, _A8, _A9, _A10, _A11, _A12, _A13, _A14, _A15, _A16, _A17,
    _A18, _A19, _A20
);
