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

use crate::externals::function::{
    FunctionDefinition, HostFunctionDefinition, VMDynamicFunction, VMDynamicFunctionWithEnv,
    VMDynamicFunctionWithoutEnv, WasmFunctionDefinition,
};
use crate::{FromToNativeWasmType, Function, FunctionType, RuntimeError, Store, WasmTypeList};
use std::panic::{catch_unwind, AssertUnwindSafe};
use wasmer_types::NativeWasmType;
use wasmer_vm::{
    ExportFunction, InstanceHandle, VMContext, VMDynamicFunctionContext, VMFunctionBody,
    VMFunctionKind,
};

/// A WebAssembly function that can be called natively
/// (using the Native ABI).
pub struct NativeFunc<'a, Args = (), Rets = ()> {
    definition: FunctionDefinition,
    store: Store,
    address: *const VMFunctionBody,
    vmctx: *mut VMContext,
    arg_kind: VMFunctionKind,
    // exported: ExportFunction,
    // This is held for its reference count to the Instance.
    _instance: Option<InstanceHandle>,
    _phantom: PhantomData<(&'a (), Args, Rets)>,
}

unsafe impl<'a, Args, Rets> Send for NativeFunc<'a, Args, Rets> {}

impl<'a, Args, Rets> NativeFunc<'a, Args, Rets>
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    pub(crate) fn new_from_wasm(
        store: Store,
        address: *const VMFunctionBody,
        vmctx: *mut VMContext,
        arg_kind: VMFunctionKind,
        definition: FunctionDefinition,
    ) -> Self {
        Self {
            definition,
            store,
            address,
            vmctx,
            arg_kind,
            _instance: Some(unsafe { (*vmctx).instance_handle() }),
            _phantom: PhantomData,
        }
    }

    pub(crate) fn new_from_host(
        store: Store,
        address: *const VMFunctionBody,
        vmctx: *mut VMContext,
        arg_kind: VMFunctionKind,
        definition: FunctionDefinition,
    ) -> Self {
        Self {
            definition,
            store,
            address,
            vmctx,
            arg_kind,
            _instance: None,
            _phantom: PhantomData,
        }
    }
}

impl<'a, Args, Rets> From<&NativeFunc<'a, Args, Rets>> for ExportFunction
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: &NativeFunc<'a, Args, Rets>) -> Self {
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        Self {
            address: other.address,
            vmctx: other.vmctx,
            signature,
            kind: other.arg_kind,
        }
    }
}

impl<'a, Args, Rets> From<NativeFunc<'a, Args, Rets>> for Function
where
    Args: WasmTypeList,
    Rets: WasmTypeList,
{
    fn from(other: NativeFunc<'a, Args, Rets>) -> Self {
        let signature = FunctionType::new(Args::wasm_types(), Rets::wasm_types());
        Self {
            store: other.store,
            definition: other.definition,
            exported: ExportFunction {
                address: other.address,
                vmctx: other.vmctx,
                signature,
                kind: other.arg_kind,
            },
        }
    }
}

macro_rules! impl_native_traits {
    (  $( $x:ident ),* ) => {
        #[allow(unused_parens, non_snake_case)]
        impl<'a $( , $x )*, Rets> NativeFunc<'a, ( $( $x ),* ), Rets>
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
        {
            /// Call the typed func and return results.
            pub fn call(&self, $( $x: $x, )* ) -> Result<Rets, RuntimeError> {
                match self.definition {
                    FunctionDefinition::Wasm(WasmFunctionDefinition {
                        trampoline,
                        module: _,
                    }) => {
                        // TODO: when `const fn` related features mature more, we can declare a single array
                        // of the correct size here.
                        let mut params_list = [ $( $x.to_native().to_binary() ),* ];
                        let mut rets_list_array = Rets::empty_array();
                        let rets_list = rets_list_array.as_mut();
                        let using_rets_array;
                        let args_rets: &mut [i128] = if params_list.len() > rets_list.len() {
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
                                self.vmctx,
                                trampoline,
                                self.address,
                                args_rets.as_mut_ptr() as *mut u8,
                            )
                        }?;
                        let num_rets = rets_list.len();
                        if !using_rets_array && num_rets > 0 {
                            let src_pointer = params_list.as_ptr();
                            let rets_list = &mut rets_list_array.as_mut()[0] as *mut i128;
                            unsafe {
                                // TODO: we can probably remove this copy by doing some clever `transmute`s.
                                // we know it's not overlapping because `using_rets_array` is false
                                std::ptr::copy_nonoverlapping(src_pointer,
                                                              rets_list,
                                                              num_rets);
                            }
                        }
                        Ok(Rets::from_array(rets_list_array))
                        // TODO: When the Host ABI and Wasm ABI are the same, we could do this instead:
                        // but we can't currently detect whether that's safe.
                        //
                        // let results = unsafe {
                        //     wasmer_vm::catch_traps_with_result(self.vmctx, || {
                        //         let f = std::mem::transmute::<_, unsafe extern "C" fn( *mut VMContext, $( $x, )*) -> Rets::CStruct>(self.address);
                        //         // We always pass the vmctx
                        //         f( self.vmctx, $( $x, )* )
                        //     }).map_err(RuntimeError::from_trap)?
                        // };
                        // Ok(Rets::from_c_struct(results))

                    }
                    FunctionDefinition::Host(HostFunctionDefinition {
                        has_env
                    }) => {
                        match self.arg_kind {
                            VMFunctionKind::Static => {
                                let results = catch_unwind(AssertUnwindSafe(|| unsafe {
                                    let f = std::mem::transmute::<_, unsafe extern "C" fn( *mut VMContext, $( $x, )*) -> Rets::CStruct>(self.address);
                                    // We always pass the vmctx
                                    f( self.vmctx, $( $x, )* )
                                })).map_err(|e| RuntimeError::new(format!("{:?}", e)))?;
                                Ok(Rets::from_c_struct(results))
                            },
                            VMFunctionKind::Dynamic => {
                                let params_list = [ $( $x.to_native().to_value() ),* ];
                                let results = if !has_env {
                                    type VMContextWithoutEnv = VMDynamicFunctionContext<VMDynamicFunctionWithoutEnv>;
                                    let ctx = self.vmctx as *mut VMContextWithoutEnv;
                                    unsafe { (*ctx).ctx.call(&params_list)? }
                                } else {
                                    type VMContextWithEnv = VMDynamicFunctionContext<VMDynamicFunctionWithEnv<std::ffi::c_void>>;
                                    let ctx = self.vmctx as *mut VMContextWithEnv;
                                    unsafe { (*ctx).ctx.call(&params_list)? }
                                };
                                let mut rets_list_array = Rets::empty_array();
                                let mut_rets = rets_list_array.as_mut() as *mut [i128] as *mut i128;
                                for (i, ret) in results.iter().enumerate() {
                                    unsafe {
                                        ret.write_value_to(mut_rets.add(i));
                                    }
                                }
                                Ok(Rets::from_array(rets_list_array))
                            }
                        }
                    },
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
