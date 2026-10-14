//! Hand-written stand-in for the import shims `wasixv2-codegen` will emit: decode wasm32 arguments,
//! call the `Syscalls` method, then take the return-to-guest path. Covers only the first slice.

use std::sync::Arc;

use wasixv2_syscalls::{Caller, Control, Fd, Syscalls, TaskId, UserSlice, UserSliceMut};
use wasmer::{AsyncFunctionEnvMut, Function, FunctionEnv, Imports, RuntimeError, Store};

use crate::{ExecEnv, IMPORT_MODULE, memory::GuestMemory, return_to_guest};

pub(crate) fn imports<S>(store: &mut Store, env: &FunctionEnv<ExecEnv<S>>) -> Imports
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    let mut imports = Imports::new();
    imports.define(
        IMPORT_MODULE,
        "read",
        Function::new_typed_with_env_async(store, env, read::<S>),
    );
    imports.define(
        IMPORT_MODULE,
        "write",
        Function::new_typed_with_env_async(store, env, write::<S>),
    );
    imports.define(
        IMPORT_MODULE,
        "exit-group",
        Function::new_typed_with_env_async(store, env, exit_group::<S>),
    );
    imports
}

async fn caller_parts<S: 'static>(env: &AsyncFunctionEnvMut<ExecEnv<S>>) -> (Arc<S>, TaskId) {
    let env = env.read().await;
    (env.data().syscalls.clone(), env.data().task)
}

async fn read<S>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    fd: i32,
    buf: i32,
    len: i32,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    let (syscalls, task) = caller_parts(&env).await;
    let memory = GuestMemory { env: &env };
    let caller = Caller::new(task, &memory);
    let ret = syscalls
        .read(&caller, Fd(fd), UserSliceMut::new(buf as u32, len as u32))
        .await;
    return_to_guest(&*syscalls, task, ret.map(|n| n as i32))
}

async fn write<S>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    fd: i32,
    buf: i32,
    len: i32,
) -> Result<i32, RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    let (syscalls, task) = caller_parts(&env).await;
    let memory = GuestMemory { env: &env };
    let caller = Caller::new(task, &memory);
    let ret = syscalls
        .write(&caller, Fd(fd), UserSlice::new(buf as u32, len as u32))
        .await;
    return_to_guest(&*syscalls, task, ret.map(|n| n as i32))
}

async fn exit_group<S>(
    env: AsyncFunctionEnvMut<ExecEnv<S>>,
    status: i32,
) -> Result<(), RuntimeError>
where
    S: Syscalls + Control + 'static,
{
    let (syscalls, task) = caller_parts(&env).await;
    let memory = GuestMemory { env: &env };
    let caller = Caller::new(task, &memory);
    let ret = syscalls.exit_group(&caller, status).await;
    return_to_guest(&*syscalls, task, ret.map(|()| 0))?;
    Err(RuntimeError::new("`exit-group` returned to the guest"))
}
