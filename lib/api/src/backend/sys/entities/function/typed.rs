#[cfg(feature = "experimental-async")]
use crate::{AsStoreAsync, StoreAsync, Value, backend::sys::async_runtime::AsyncCallFuture};
use crate::{
    AsStoreMut, CallWrapper, FromToNativeWasmType, Function, NativeWasmTypeInto, RuntimeError,
    TypedFunction, WasmTypeList,
};
use std::future::Future;
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
            pub fn call_sys(&self, store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                self.call_wrapped_sys(store, None, $( $x, )*)
            }

            /// Call the typed func through `wrapper` and return results.
            #[allow(clippy::too_many_arguments)]
            pub(crate) fn call_wrapped_sys(&self, store: &mut impl AsStoreMut, wrapper: Option<&mut dyn CallWrapper>, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                let mut params = Self::sys_arguments(store, $( $x, )*)?;
                let mut results = Rets::empty_array();
                invoke(&self.func, store, wrapper, params.as_mut(), results.as_mut())?;
                Ok(unsafe { Rets::from_array(store, results) })
            }

            #[allow(clippy::too_many_arguments)]
            fn sys_arguments(
                store: &mut impl AsStoreMut, $( $x: $x, )*
            ) -> Result<<($( $x ),*) as WasmTypeList>::Array, RuntimeError> {
                if $(!FromToNativeWasmType::is_from_store(&$x, store) ||)* false {
                    return Err(RuntimeError::new("cross-`Store` values are not supported"));
                }
                Ok([$( $x.to_native().into_raw(store) ),*])
            }

            /// Call the typed func asynchronously.
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            #[cfg(feature = "experimental-async")]
            pub(crate) fn call_async_sys(
                func: Function,
                store: StoreAsync,
                $( $x: $x, )*
            ) -> impl Future<Output = Result<Rets, RuntimeError>> + 'static
            where
                $( $x: FromToNativeWasmType + 'static, )*
            {
                async move {
                    let (mut params, mut results) = {
                        let mut write = store.write_lock().await;
                        let mut params = Self::sys_arguments(&mut write, $( $x, )*)?;
                        let signature = func.as_sys().signature(&write);
                        let num_results = signature.results().len();
                        if signature.params().iter().chain(signature.results()).any(|ty| ty.is_ref()) {
                            // Converting a funcref can grow the store's function table.
                            let types = signature.params().to_vec();
                            let values = params.as_mut().iter().zip(types).map(|(&raw, ty)| {
                                unsafe { Value::from_raw(&mut write, ty, raw) }
                            }).collect();
                            drop(write);
                            return call_with_references::<Rets>(func, &store, values).await;
                        }
                        (params, AsyncResults::new(num_results))
                    };
                    let mut results = AsyncCallFuture::new(store.store(), move |store| {
                        invoke(&func, store, None, params.as_mut(), results.slots())?;
                        Ok(results)
                    }).await?;
                    let mut write = store.write_lock().await;
                    let mut array = Rets::empty_array();
                    array.as_mut().copy_from_slice(results.slots());
                    // These slots were written by this typed function's trampoline.
                    Ok(unsafe { Rets::from_array(&mut write, array) })
                }
            }

            #[doc(hidden)]
            #[allow(missing_docs)]
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call_raw_sys(&self, store: &mut impl AsStoreMut, mut params_list: Vec<RawValue> ) -> Result<Rets, RuntimeError> {
                let mut results = Rets::empty_array();
                invoke(&self.func, store, None, &mut params_list, results.as_mut())?;
                Ok(unsafe { Rets::from_array(store, results) })
            }
        }
    };
}

// Store-owned references cross an await as handles. Raw references are valid
// only while the owning store is locked.
#[cfg(feature = "experimental-async")]
async fn call_with_references<Rets: WasmTypeList>(
    func: Function,
    store: &StoreAsync,
    params: Vec<Value>,
) -> Result<Rets, RuntimeError> {
    let values = func.call_async(store, params).await?;
    let mut write = store.write_lock().await;
    let mut results = Rets::empty_array();
    let slots = results.as_mut();
    if slots.len() != values.len() {
        return Err(RuntimeError::new("result arity mismatch"));
    }
    for (slot, value) in slots.iter_mut().zip(values.iter()) {
        if !value.is_from_store(&write) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        *slot = value.as_raw(&write);
    }
    Ok(unsafe { Rets::from_array(&mut write, results) })
}

/// The trampoline overwrites the arguments with the results.
#[inline]
fn invoke(
    func: &Function,
    store: &mut impl AsStoreMut,
    wrapper: Option<&mut dyn CallWrapper>,
    params: &mut [RawValue],
    results: &mut [RawValue],
) -> Result<(), RuntimeError> {
    if params.len() > results.len() {
        func.as_sys().invoke_wasm(store, wrapper, params)?;
        results.copy_from_slice(&params[..results.len()]);
    } else {
        results[..params.len()].copy_from_slice(params);
        func.as_sys().invoke_wasm(store, wrapper, results)?;
    }
    Ok(())
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

// A coroutine owns its output, whereas the public result type need not be
// 'static. Keep the ABI values until the completing poll converts them.
#[cfg(feature = "experimental-async")]
enum AsyncResults {
    Single(RawValue),
    Other(Box<[RawValue]>),
}

#[cfg(feature = "experimental-async")]
impl AsyncResults {
    fn new(len: usize) -> Self {
        let zero = RawValue { i32: 0 };
        match len {
            1 => Self::Single(zero),
            _ => Self::Other(vec![zero; len].into_boxed_slice()),
        }
    }

    fn slots(&mut self) -> &mut [RawValue] {
        match self {
            Self::Single(one) => std::slice::from_mut(one),
            Self::Other(other) => other,
        }
    }
}
