use std::{future::Future, marker::PhantomData, pin::Pin};

use crate::{
    FunctionEnvMut, HostFunctionKind, RuntimeError, WasmTypeList, WithEnv, WithoutEnv,
    utils::{FromToNativeWasmType, IntoResult},
};

/// Wrapper conveying whether an async host function receives an environment.
pub enum AsyncFunctionEnv<'a, T, Kind> {
    /// Used by host functions without an environment.
    WithoutEnv(PhantomData<(&'a T, Kind)>),
    /// Used by host functions that capture an environment.
    WithEnv(FunctionEnvMut<'a, T>),
}

impl<'a, T> AsyncFunctionEnv<'a, T, WithoutEnv> {
    /// Create an environment wrapper for functions without host state.
    pub fn new() -> Self {
        Self::WithoutEnv(PhantomData)
    }
}

impl<'a, T> AsyncFunctionEnv<'a, T, WithEnv> {
    /// Create an environment wrapper carrying [`FunctionEnvMut`].
    pub fn with_env(env: FunctionEnvMut<'a, T>) -> Self {
        Self::WithEnv(env)
    }

    /// Extract the underlying [`FunctionEnvMut`].
    pub fn into_env(self) -> FunctionEnvMut<'a, T> {
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
        env: AsyncFunctionEnv<'_, T, Kind>,
        args: Args,
    ) -> Pin<Box<dyn Future<Output = Result<Rets, RuntimeError>> + Send>>;
}

macro_rules! impl_async_host_function_without_env {
    ( $( $x:ident ),* ) => {
        impl<$( $x, )* Rets, RetsAsResult, F, Fut > AsyncHostFunction<(), ( $( $x ),* ), Rets, WithoutEnv> for F
        where
            F: Fn($( $x ),*) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = RetsAsResult> + Send + 'static,
            RetsAsResult: IntoResult<Rets>,
            Rets: WasmTypeList,
            ( $( $x ),* ): WasmTypeList + 'static,
            $( $x: FromToNativeWasmType + 'static, )*
        {
            fn call_async(
                &self,
                _env: AsyncFunctionEnv<'_, (), WithoutEnv>,
                args: ( $( $x ),* ),
            ) -> Pin<Box<dyn Future<Output = Result<Rets, RuntimeError>> + Send>> {
                #[allow(non_snake_case)]
                let ( $( $x ),* ) = args;
                let fut = (self)( $( $x ),* );
                Box::pin(async move {
                    fut.await
                        .into_result()
                        .map_err(|err| RuntimeError::user(Box::new(err)))
                })
            }
        }
    };
}

macro_rules! impl_async_host_function_with_env {
    ( $( $x:ident ),* ) => {
        impl<$( $x, )* Rets, RetsAsResult,  T, F, Fut > AsyncHostFunction<T, ( $( $x ),* ), Rets, WithEnv> for F
        where
            T: Send + 'static,
            F: Fn(FunctionEnvMut<'_, T>, $( $x ),*) -> Fut + Send  + 'static,
            Fut: Future<Output = RetsAsResult> + Send + 'static,
            RetsAsResult: IntoResult<Rets>,
            Rets: WasmTypeList,
            ( $( $x ),* ): WasmTypeList + 'static,
            $( $x: FromToNativeWasmType + 'static, )*
        {
            fn call_async(
                &self,
                env: AsyncFunctionEnv<'_, T, WithEnv>,
                args: ( $( $x ),* ),
            ) -> Pin<Box<dyn Future<Output = Result<Rets, RuntimeError>> + Send>> {
                #[allow(non_snake_case)]
                let ( $( $x ),* ) = args;
                let fut = (self)(env.into_env(), $( $x ),* );
                Box::pin(async move {
                    fut.await
                        .into_result()
                        .map_err(|err| RuntimeError::user(Box::new(err)))
                })
            }
        }
    };
}

impl_async_host_function_without_env!();
impl_async_host_function_without_env!(A1);
impl_async_host_function_without_env!(A1, A2);
impl_async_host_function_without_env!(A1, A2, A3);
impl_async_host_function_without_env!(A1, A2, A3, A4);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7, A8);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_async_host_function_without_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
impl_async_host_function_without_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15
);
impl_async_host_function_without_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16
);
impl_async_host_function_without_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17
);
impl_async_host_function_without_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18
);
impl_async_host_function_without_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19
);
impl_async_host_function_without_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20
);

impl_async_host_function_with_env!();
impl_async_host_function_with_env!(A1);
impl_async_host_function_with_env!(A1, A2);
impl_async_host_function_with_env!(A1, A2, A3);
impl_async_host_function_with_env!(A1, A2, A3, A4);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7, A8);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_async_host_function_with_env!(A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14);
impl_async_host_function_with_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15
);
impl_async_host_function_with_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16
);
impl_async_host_function_with_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17
);
impl_async_host_function_with_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18
);
impl_async_host_function_with_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19
);
impl_async_host_function_with_env!(
    A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20
);
