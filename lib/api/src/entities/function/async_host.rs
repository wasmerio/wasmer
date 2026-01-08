use std::{future::Future, marker::PhantomData, pin::Pin};

use crate::{
    AsyncFunctionEnvMut, HostFunctionKind, RuntimeError, WasmTypeList, WithEnv, WithoutEnv,
    utils::{FromToNativeWasmType, IntoResult},
};

/// Wrapper conveying whether an async host function receives an environment.
pub enum AsyncFunctionEnv<T, Kind> {
    /// Used by host functions without an environment.
    WithoutEnv(PhantomData<(T, Kind)>),
    /// Used by host functions that capture an environment.
    WithEnv(AsyncFunctionEnvMut<T>),
}

impl<T> AsyncFunctionEnv<T, WithoutEnv> {
    /// Create an environment wrapper for functions without host state.
    pub fn new() -> Self {
        Self::WithoutEnv(PhantomData)
    }
}

impl<T> AsyncFunctionEnv<T, WithEnv> {
    /// Create an environment wrapper carrying [`AsyncFunctionEnvMut`].
    pub fn with_env(env: AsyncFunctionEnvMut<T>) -> Self {
        Self::WithEnv(env)
    }

    /// Extract the underlying [`AsyncFunctionEnvMut`].
    pub fn into_env(self) -> AsyncFunctionEnvMut<T> {
        match self {
            Self::WithEnv(env) => env,
            Self::WithoutEnv(_) => unreachable!("with-env async function called without env"),
        }
    }
}

/// Async counterpart to [`HostFunction`](super::host::HostFunction).
pub trait AsyncHostFunction<T, Args, Rets, Kind>
where
    Args: WasmTypeList + 'static,
    Rets: WasmTypeList,
    Kind: HostFunctionKind,
{
    /// Invoke the host function asynchronously.
    fn call_async(
        &self,
        env: AsyncFunctionEnv<T, Kind>,
        args: Args,
    ) -> Pin<Box<dyn Future<Output = Result<Rets, RuntimeError>>>>;
}

macro_rules! impl_async_host_function {
    ([$c_struct_representation:ident] $c_struct_name:ident, $( $x:ident ),* ) => {
        impl<$( $x, )* Rets, RetsAsResult, F, Fut > AsyncHostFunction<(), ( $( $x ),* ), Rets, WithoutEnv> for F
        where
            F: Fn($( $x ),*) -> Fut + 'static,
            Fut: Future<Output = RetsAsResult> + 'static,
            RetsAsResult: IntoResult<Rets>,
            Rets: WasmTypeList,
            ( $( $x ),* ): WasmTypeList + 'static,
            $( $x: FromToNativeWasmType + 'static, )*
        {
            fn call_async(
                &self,
                _env: AsyncFunctionEnv<(), WithoutEnv>,
                args: ( $( $x ),* ),
            ) -> Pin<Box<dyn Future<Output = Result<Rets, RuntimeError>>>> {
                #[allow(non_snake_case)]
                let ( $( $x ),* ) = args;
                let fut = (self)( $( $x ),* );
                Box::pin(async move {
                    fut.await
                        .into_result()
                        .map_err(|err| RuntimeError::from_dyn(Box::new(err)))
                })
            }
        }

        impl<$( $x, )* Rets, RetsAsResult,  T, F, Fut > AsyncHostFunction<T, ( $( $x ),* ), Rets, WithEnv> for F
        where
            T: 'static,
            F: Fn(AsyncFunctionEnvMut<T>, $( $x ),*) -> Fut + 'static,
            Fut: Future<Output = RetsAsResult> + 'static,
            RetsAsResult: IntoResult<Rets>,
            Rets: WasmTypeList,
            ( $( $x ),* ): WasmTypeList + 'static,
            $( $x: FromToNativeWasmType + 'static, )*
        {
            fn call_async(
                &self,
                env: AsyncFunctionEnv<T, WithEnv>,
                args: ( $( $x ),* ),
            ) -> Pin<Box<dyn Future<Output = Result<Rets, RuntimeError>>>> {
                #[allow(non_snake_case)]
                let ( $( $x ),* ) = args;
                let fut = (self)(env.into_env(), $( $x ),* );
                Box::pin(async move {
                    fut.await
                        .into_result()
                        .map_err(|err| RuntimeError::from_dyn(Box::new(err)))
                })
            }
        }
    };
}

impl_async_host_function!([C] S0,);
impl_async_host_function!([transparent] S1, A1);
impl_async_host_function!([C] S2, A1, A2);
impl_async_host_function!([C] S3, A1, A2, A3);
impl_async_host_function!([C] S4, A1, A2, A3, A4);
impl_async_host_function!([C] S5, A1, A2, A3, A4, A5);
impl_async_host_function!([C] S6, A1, A2, A3, A4, A5, A6);
impl_async_host_function!([C] S7, A1, A2, A3, A4, A5, A6, A7);
impl_async_host_function!([C] S8, A1, A2, A3, A4, A5, A6, A7, A8);
impl_async_host_function!([C] S9, A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_async_host_function!([C] S10, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_async_host_function!([C] S11, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_async_host_function!([C] S12, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_async_host_function!([C] S13, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_async_host_function!([C] S14, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
impl_async_host_function!([C] S15, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15);
impl_async_host_function!([C] S16, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16);
impl_async_host_function!([C] S17, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17);
impl_async_host_function!([C] S18, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18);
impl_async_host_function!([C] S19, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19);
impl_async_host_function!([C] S20, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20);
impl_async_host_function!([C] S21, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21);
impl_async_host_function!([C] S22, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22);
impl_async_host_function!([C] S23, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23);
impl_async_host_function!([C] S24, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24);
impl_async_host_function!([C] S25, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25);
impl_async_host_function!([C] S26, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26);
