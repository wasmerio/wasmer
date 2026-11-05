//! Typed import invocation using the same argument roles as signature validation.

use std::{future::Future, sync::Arc};

use wasixv2_exec::ExecEnv;
use wasixv2_syscalls::{Control, LocalFuture, Syscalls};
use wasmer::{Function, FunctionEnv, RuntimeError, Store, Value};

use crate::{
    abi::{AbiWidth, GuestSize, Pointer},
    imports::{ArgumentRole, Entry, Module},
    shim::Env,
};

trait Argument: Sized + 'static {
    const ROLE: ArgumentRole;
    fn decode(value: &Value, role: ArgumentRole, width: AbiWidth) -> Self;
}

impl Argument for i32 {
    const ROLE: ArgumentRole = ArgumentRole::I32;
    fn decode(value: &Value, role: ArgumentRole, _: AbiWidth) -> Self {
        assert_eq!(role, Self::ROLE);
        value.i32().expect("validated i32 import argument")
    }
}

impl Argument for i64 {
    const ROLE: ArgumentRole = ArgumentRole::I64;
    fn decode(value: &Value, role: ArgumentRole, _: AbiWidth) -> Self {
        assert_eq!(role, Self::ROLE);
        value.i64().expect("validated i64 import argument")
    }
}

fn word(value: &Value, width: AbiWidth) -> u64 {
    match width {
        AbiWidth::Wasm32 => u64::from(value.i32().expect("validated i32 word") as u32),
        AbiWidth::Wasm64 => value.i64().expect("validated i64 word") as u64,
    }
}

impl Argument for Pointer {
    const ROLE: ArgumentRole = ArgumentRole::Pointer;
    fn decode(value: &Value, role: ArgumentRole, width: AbiWidth) -> Self {
        assert_eq!(role, Self::ROLE);
        Self(word(value, width))
    }
}

impl Argument for GuestSize {
    const ROLE: ArgumentRole = ArgumentRole::Size;
    fn decode(value: &Value, role: ArgumentRole, width: AbiWidth) -> Self {
        assert_eq!(role, Self::ROLE);
        Self(word(value, width))
    }
}

pub(crate) trait Results {
    fn values(self) -> Vec<Value>;
}

impl Results for i32 {
    fn values(self) -> Vec<Value> {
        vec![Value::I32(self)]
    }
}

impl Results for () {
    fn values(self) -> Vec<Value> {
        Vec::new()
    }
}

pub(crate) trait ImportFunction<S: 'static, Args, R>: 'static {
    fn roles() -> Vec<ArgumentRole>;
    fn invoke(
        &self,
        env: Env<S>,
        values: &[Value],
        roles: &[ArgumentRole],
    ) -> LocalFuture<'static, Result<Vec<Value>, RuntimeError>>;
}

macro_rules! functions {
    ($($arg:ident : $index:tt),*) => {
        impl<S, F, Fut, R, $($arg,)*> ImportFunction<S, ($($arg,)*), R> for F
        where
            S: 'static,
            F: Fn(Env<S>, $($arg),*) -> Fut + 'static,
            Fut: Future<Output = Result<R, RuntimeError>> + 'static,
            R: Results + 'static,
            $($arg: Argument,)*
        {
            fn roles() -> Vec<ArgumentRole> {
                vec![$($arg::ROLE),*]
            }

            #[allow(non_snake_case)]
            fn invoke(
                &self,
                env: Env<S>,
                values: &[Value],
                roles: &[ArgumentRole],
            ) -> LocalFuture<'static, Result<Vec<Value>, RuntimeError>> {
                let _ = (&values, &roles);
                $(let $arg = $arg::decode(&values[$index], roles[$index], env.width());)*
                let future = self(env, $($arg),*);
                Box::pin(async move { future.await.map(Results::values) })
            }
        }
    };
}

functions!();
functions!(A: 0);
functions!(A: 0, B: 1);
functions!(A: 0, B: 1, C: 2);
functions!(A: 0, B: 1, C: 2, D: 3);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6, H: 7);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6, H: 7, I: 8);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6, H: 7, I: 8, J: 9);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6, H: 7, I: 8, J: 9, K: 10);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6, H: 7, I: 8, J: 9, K: 10, L: 11);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6, H: 7, I: 8, J: 9, K: 10, L: 11, M: 12);
functions!(A: 0, B: 1, C: 2, D: 3, E: 4, F0: 5, G: 6, H: 7, I: 8, J: 9, K: 10, L: 11, M: 12, N: 13);

pub(crate) fn function<S, F, Args, R>(
    store: &mut Store,
    env: &FunctionEnv<ExecEnv<S>>,
    module: Module,
    entry: &'static Entry,
    function: F,
) -> Function
where
    S: Syscalls + Control + 'static,
    F: ImportFunction<S, Args, R>,
    R: Results + 'static,
{
    assert_eq!(F::roles(), entry.roles, "roles for {}", entry.name);
    let function = Arc::new(function);
    Function::new_with_env_async(store, env, entry.signature(module), move |env, values| {
        function.invoke(Env::new(env, module.width()), values, entry.roles)
    })
}
