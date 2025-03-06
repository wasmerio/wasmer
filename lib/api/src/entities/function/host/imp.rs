use crate::{
    utils::{FromToNativeWasmType, IntoResult, NativeWasmTypeInto, WasmTypeList},
    AsStoreMut, AsStoreRef, BackendFunctionEnv, BackendFunctionEnvMut, FunctionEnvMut,
    FunctionType, HostFunction, RuntimeError, StoreInner, StoreMut, StoreRef, Value, WithEnv,
    WithoutEnv,
};

use std::panic::{self, AssertUnwindSafe};
use std::{cell::UnsafeCell, cmp::max, ffi::c_void};
use wasmer_types::{NativeWasmType, RawValue};

macro_rules! impl_host_function {
    ([$c_struct_representation:ident] $c_struct_name:ident, $( $x:ident ),* ) => {
        #[allow(unused_parens)]
        impl< $( $x, )* Rets, RetsAsResult, T, Func> crate::HostFunction<T, ( $( $x ),* ), Rets, WithEnv> for Func where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
            RetsAsResult: IntoResult<Rets>,
            T: Send + 'static,
            Func: Fn(FunctionEnvMut<'_, T>, $( $x , )*) -> RetsAsResult + 'static,
        {
            #[allow(non_snake_case)]
            fn function_callback(&self, rt: crate::backend::BackendKind) -> crate::vm::VMFunctionCallback {
                paste::paste! {
                    match rt {
                        #[cfg(feature = "sys")]
                        crate::backend::BackendKind::Headless => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "llvm")]
                        crate::backend::BackendKind::LLVM => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "cranelift")]
                        crate::backend::BackendKind::Cranelift => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "singlepass")]
                        crate::backend::BackendKind::Singlepass => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "js")]
                        crate::backend::BackendKind::Js => crate::vm::VMFunctionCallback::Js(crate::backend::js::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "jsc")]
                        crate::backend::BackendKind::Jsc => crate::vm::VMFunctionCallback::Jsc(crate::backend::jsc::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "wamr")]
                        crate::backend::BackendKind::Wamr => crate::vm::VMFunctionCallback::Wamr(crate::backend::wamr::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "wasmi")]
                        crate::backend::BackendKind::Wasmi => crate::vm::VMFunctionCallback::Wasmi(crate::backend::wasmi::function::[<gen_fn_callback_ $c_struct_name:lower >](self)),
                        #[cfg(feature = "v8")]
                        crate::backend::BackendKind::V8 => crate::vm::VMFunctionCallback::V8(crate::backend::v8::function::[<gen_fn_callback_ $c_struct_name:lower >](self))
                    }
                }
            }

            #[cfg(feature = "sys")]
            fn function_callback_sys(&self) -> crate::vm::VMFunctionCallback {
                paste::paste! {
                    crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower >](self))
                }
            }

            #[allow(non_snake_case)]
            fn call_trampoline_address() -> crate::vm::VMTrampoline {
                paste::paste! {
                    #[cfg(feature = "sys")]
                    {
                        crate::vm::VMTrampoline::Sys(crate::backend::sys::function::[<gen_call_trampoline_address_ $c_struct_name:lower >]::<$($x,)* Rets>())
                    }

                    #[cfg(not(feature = "sys"))]
                    {
                        unimplemented!("No backend available")
                    }
                }
            }
        }

        // Implement `HostFunction` for a function that has the same arity than the tuple.
        #[allow(unused_parens)]
        impl< $( $x, )* Rets, RetsAsResult, Func >
            crate::HostFunction<(), ( $( $x ),* ), Rets, WithoutEnv>
        for
            Func
        where
            $( $x: FromToNativeWasmType, )*
            Rets: WasmTypeList,
            RetsAsResult: IntoResult<Rets>,
            Func: Fn($( $x , )*) -> RetsAsResult + 'static
        {
            #[allow(non_snake_case)]
            fn function_callback(&self, rt: crate::backend::BackendKind) -> crate::vm::VMFunctionCallback {
                paste::paste! {
                    match rt {
                        #[cfg(feature = "sys")]
                        crate::backend::BackendKind::Headless => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "llvm")]
                        crate::backend::BackendKind::LLVM => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "cranelift")]
                        crate::backend::BackendKind::Cranelift => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "singlepass")]
                        crate::backend::BackendKind::Singlepass => crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "js")]
                        crate::backend::BackendKind::Js => crate::vm::VMFunctionCallback::Js(crate::backend::js::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "jsc")]
                        crate::backend::BackendKind::Jsc => crate::vm::VMFunctionCallback::Jsc(crate::backend::jsc::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "wamr")]
                        crate::backend::BackendKind::Wamr => crate::vm::VMFunctionCallback::Wamr(crate::backend::wamr::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "wasmi")]
                        crate::backend::BackendKind::Wasmi => crate::vm::VMFunctionCallback::Wasmi(crate::backend::wasmi::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self)),
                        #[cfg(feature = "v8")]
                        crate::backend::BackendKind::V8 => crate::vm::VMFunctionCallback::V8(crate::backend::v8::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self))
                    }
                }
            }

            #[allow(non_snake_case)]
            #[cfg(feature = "sys")]
            fn function_callback_sys(&self) -> crate::vm::VMFunctionCallback {
                paste::paste! {
                    crate::vm::VMFunctionCallback::Sys(crate::backend::sys::function::[<gen_fn_callback_ $c_struct_name:lower _no_env>](self))
                }
            }

            #[allow(non_snake_case)]
            fn call_trampoline_address() -> crate::vm::VMTrampoline {
                paste::paste! {
                    #[cfg(feature = "sys")]
                    {
                        crate::vm::VMTrampoline::Sys(crate::backend::sys::function::[<gen_call_trampoline_address_ $c_struct_name:lower _no_env>]::<$($x,)* Rets>())
                    }

                    #[cfg(not(feature = "sys"))]
                    {
                        unimplemented!("No backend available")
                    }
                }
            }
        }
    };
}

// Black-magic to count the number of identifiers at compile-time.
macro_rules! count_idents {
    ( $($idents:ident),* ) => {
        {
            #[allow(dead_code, non_camel_case_types)]
            enum Idents { $( $idents, )* __CountIdentsLast }
            const COUNT: usize = Idents::__CountIdentsLast as usize;
            COUNT
        }
    };
}

// Here we go! Let's generate all the C struct, `WasmTypeList`
// implementations and `HostFunction` implementations.
impl_host_function!([C] S0,);
impl_host_function!([transparent] S1, A1);
impl_host_function!([C] S2, A1, A2);
impl_host_function!([C] S3, A1, A2, A3);
impl_host_function!([C] S4, A1, A2, A3, A4);
impl_host_function!([C] S5, A1, A2, A3, A4, A5);
impl_host_function!([C] S6, A1, A2, A3, A4, A5, A6);
impl_host_function!([C] S7, A1, A2, A3, A4, A5, A6, A7);
impl_host_function!([C] S8, A1, A2, A3, A4, A5, A6, A7, A8);
impl_host_function!([C] S9, A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_host_function!([C] S10, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_host_function!([C] S11, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_host_function!([C] S12, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_host_function!([C] S13, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_host_function!([C] S14, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
impl_host_function!([C] S15, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
impl_host_function!([C] S16, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
impl_host_function!([C] S17, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17);
impl_host_function!([C] S18, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18);
impl_host_function!([C] S19, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19);
impl_host_function!([C] S20, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20);
impl_host_function!([C] S21, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21);
impl_host_function!([C] S22, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22);
impl_host_function!([C] S23, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23);
impl_host_function!([C] S24, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24);
impl_host_function!([C] S25, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25);
impl_host_function!([C] S26, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26);
