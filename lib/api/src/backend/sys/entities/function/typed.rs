use crate::backend::sys::engine::NativeEngineExt;
use crate::{
    FromToNativeWasmType, Function, NativeWasmTypeInto, RuntimeError, StoreContext, TypedFunction,
    Value, WasmTypeList,
    store::{AsStoreMut, AsStoreRef},
};
#[cfg(feature = "experimental-async")]
use crate::{StoreAsync, store::AsStoreAsync};
use std::future::Future;
use wasmer_types::{FunctionType, RawValue, Type};

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
            pub fn call_sys(&self, store: &mut impl AsStoreMut, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                let anyfunc = unsafe {
                    *self.func.as_sys()
                        .handle
                        .get(store.as_store_ref().objects().as_sys())
                        .anyfunc
                        .as_ptr()
                        .as_ref()
                };
                // Ensure all parameters come from the same context.
                if $(!FromToNativeWasmType::is_from_store(&$x, store) ||)* false {
                    return Err(RuntimeError::new(
                        "cross-`Store` values are not supported",
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

                let store_id = store.objects_mut().id();
                // Install the store into the store context
                let store_install_guard = unsafe {
                    StoreContext::ensure_installed(store.as_store_mut().inner as *mut _)
                };

                let mut r;
                loop {
                    let storeref = store.as_store_ref();
                    let config = storeref.engine().tunables().vmconfig();
                    r = unsafe {
                        // Safety: This is the intended use-case for StoreContext::pause, as
                        // documented in the function's doc comments.
                        let pause_guard = StoreContext::pause(store_id);
                        wasmer_vm::wasmer_call_trampoline(
                            store.as_store_ref().signal_handler(),
                            config,
                            anyfunc.vmctx,
                            anyfunc.call_trampoline,
                            anyfunc.func_ptr,
                            args_rets.as_mut_ptr() as *mut u8,
                        )
                    };
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

                drop(store_install_guard);

                r?;

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
                    let mut write = store.write_lock().await;
                    let func_ty = func.ty(&mut write);
                    let mut params_raw = [ $( $x.to_native().into_raw(&mut write) ),* ];
                    let mut params_values = Vec::with_capacity(params_raw.len());
                    {
                        for (raw, ty) in params_raw.iter().zip(func_ty.params()) {
                            unsafe {
                                params_values.push(Value::from_raw(&mut write, *ty, *raw));
                            }
                        }
                    }
                    drop(write);

                    let results = func.call_async(&store, params_values).await?;
                    let mut write = store.write_lock().await;
                    convert_results::<Rets>(&mut write, func_ty, &results)
                }
            }

            #[doc(hidden)]
            #[allow(missing_docs)]
            #[allow(unused_mut)]
            #[allow(clippy::too_many_arguments)]
            pub fn call_raw_sys(&self, store: &mut impl AsStoreMut, mut params_list: Vec<RawValue> ) -> Result<Rets, RuntimeError> {
                let anyfunc = unsafe {
                    *self.func.as_sys()
                        .handle
                        .get(store.as_store_ref().objects().as_sys())
                        .anyfunc
                        .as_ptr()
                        .as_ref()
                };
                // TODO: when `const fn` related features mature more, we can declare a single array
                // of the correct size here.
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

                let store_id = store.objects_mut().id();
                // Install the store into the store context
                let store_install_guard = unsafe {
                    StoreContext::ensure_installed(store.as_store_mut().inner as *mut _)
                };

                let mut r;
                loop {
                    let storeref = store.as_store_ref();
                    let config = storeref.engine().tunables().vmconfig();
                    r = unsafe {
                        // Safety: This is the intended use-case for StoreContext::pause, as
                        // documented in the function's doc comments.
                        let pause_guard = StoreContext::pause(store_id);
                        wasmer_vm::wasmer_call_trampoline(
                            store.as_store_ref().signal_handler(),
                            config,
                            anyfunc.vmctx,
                            anyfunc.call_trampoline,
                            anyfunc.func_ptr,
                            args_rets.as_mut_ptr() as *mut u8,
                        )
                    };
                    let store_mut = store.as_store_mut();
                    if let Some(callback) = store_mut.inner.on_called.take() {
                        // TODO: OnCalledAction is needed for asyncify. It will be refactored with https://github.com/wasmerio/wasmer/issues/3451
                        match callback(store_mut) {
                            Ok(wasmer_types::OnCalledAction::InvokeAgain) => { continue; }
                            Ok(wasmer_types::OnCalledAction::Finish) => { break; }
                            Ok(wasmer_types::OnCalledAction::Trap(trap)) => { return Err(RuntimeError::user(trap)) },
                            Err(trap) => { return Err(RuntimeError::user(trap)) },
                        }
                    }
                    break;
                }

                drop(store_install_guard);

                r?;

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

fn convert_results<Rets>(
    store: &mut impl AsStoreMut,
    ty: FunctionType,
    results: &[Value],
) -> Result<Rets, RuntimeError>
where
    Rets: WasmTypeList,
{
    if results.len() != ty.results().len() {
        return Err(RuntimeError::new("result arity mismatch"));
    }
    let mut raw_array = Rets::empty_array();
    for ((slot, value_ty), value) in raw_array
        .as_mut()
        .iter_mut()
        .zip(ty.results().iter())
        .zip(results.iter())
    {
        debug_assert_eq!(value.ty(), *value_ty);
        *slot = value.as_raw(store);
    }
    unsafe { Ok(Rets::from_array(store, raw_array)) }
}
