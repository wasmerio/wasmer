//! V1 function imports select their existing buffer conventions before layered dispatch.

use crate::shim::{self, Env};
use wasixv2_abi::{CallDynamicFlags, GuestPtrMut};
use wasixv2_exec::{CallBufferPolicy, GuestFunctionOperations};
use wasixv2_syscalls::{Control, Syscalls, UserSlice, UserSliceMut};
use wasmer::RuntimeError;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn call_dynamic<S: Syscalls + Control + 'static>(
    env: Env<S>,
    function: i32,
    values: i32,
    values_len: i32,
    results: i32,
    results_len: i32,
    strict: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let functions = GuestFunctionOperations::new(frame.env, frame.unwind()).with_buffer_policy(
            CallBufferPolicy {
                strict_results_before_call: false,
                zero_result_tail: false,
                whole_result_fields: true,
            },
        );
        let caller = frame.caller().with_functions(&functions);
        let flags = if strict as u8 == 1 {
            CallDynamicFlags::CALL_DYNAMIC_STRICT
        } else {
            CallDynamicFlags::empty()
        };
        let result = frame
            .syscalls
            .call_dynamic(
                &caller,
                function as u32,
                UserSlice::new(values as u32, values_len as u32),
                UserSliceMut::new(results as u32, results_len as u32),
                flags,
            )
            .await?;
        extension_result(result)
    })
    .await
}

pub(crate) async fn reflect_signature<S: Syscalls + Control + 'static>(
    env: Env<S>,
    function: i32,
    arguments: i32,
    arguments_len: i32,
    results: i32,
    results_len: i32,
    result: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let functions = GuestFunctionOperations::new(frame.env, frame.unwind());
        let caller = frame.caller().with_functions(&functions);
        let result = frame
            .syscalls
            .reflect_signature(
                &caller,
                function as u32,
                UserSliceMut::new(arguments as u32, u32::from(arguments_len as u16)),
                UserSliceMut::new(results as u32, u32::from(results_len as u16)),
                GuestPtrMut::new(result as u32),
            )
            .await?;
        extension_result(result)
    })
    .await
}

pub(crate) async fn closure_allocate<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let functions = GuestFunctionOperations::new(frame.env, frame.unwind());
        let caller = frame.caller().with_functions(&functions);
        let result = frame
            .syscalls
            .closure_allocate(&caller, GuestPtrMut::new(out as u32))
            .await?;
        extension_result(result)
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn closure_prepare<S: Syscalls + Control + 'static>(
    env: Env<S>,
    backing: i32,
    closure: i32,
    arguments: i32,
    arguments_len: i32,
    results: i32,
    results_len: i32,
    environment: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let functions = GuestFunctionOperations::new(frame.env, frame.unwind());
        let caller = frame.caller().with_functions(&functions);
        let result = frame
            .syscalls
            .closure_prepare(
                &caller,
                backing as u32,
                closure as u32,
                UserSlice::new(arguments as u32, arguments_len as u32),
                UserSlice::new(results as u32, results_len as u32),
                environment as u32,
            )
            .await?;
        extension_result(result)
    })
    .await
}

pub(crate) async fn closure_free<S: Syscalls + Control + 'static>(
    env: Env<S>,
    closure: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let functions =
            GuestFunctionOperations::new(frame.env, frame.unwind()).with_idempotent_closure_free();
        let caller = frame.caller().with_functions(&functions);
        let result = frame.syscalls.closure_free(&caller, closure as u32).await?;
        extension_result(result)
    })
    .await
}

pub(crate) fn context_create<S: Syscalls + Control + 'static>(
    env: Env<S>,
    out: i32,
    entry: i32,
) -> impl std::future::Future<Output = Result<i32, RuntimeError>> + 'static {
    let contexts_supported = wasixv2_exec::context_operations_supported();
    async move {
        shim::errno(&env, async |frame| {
            let functions = GuestFunctionOperations::new(frame.env, frame.unwind())
                .with_context_support(contexts_supported);
            let caller = frame.caller().with_functions(&functions);
            let result = frame
                .syscalls
                .context_create(&caller, entry as u32, GuestPtrMut::new(out as u32))
                .await?;
            extension_result(result)
        })
        .await
    }
}

pub(crate) fn context_switch<S: Syscalls + Control + 'static>(
    env: Env<S>,
    id: i64,
) -> impl std::future::Future<Output = Result<i32, RuntimeError>> + 'static {
    let contexts_supported = wasixv2_exec::context_operations_supported();
    async move {
        shim::errno(&env, async |frame| {
            let functions = GuestFunctionOperations::new(frame.env, frame.unwind())
                .with_context_support(contexts_supported);
            let caller = frame.caller().with_functions(&functions);
            let result = frame.syscalls.context_switch(&caller, id as u64).await?;
            extension_result(result)
        })
        .await
    }
}

pub(crate) fn context_destroy<S: Syscalls + Control + 'static>(
    env: Env<S>,
    id: i64,
) -> impl std::future::Future<Output = Result<i32, RuntimeError>> + 'static {
    let contexts_supported = wasixv2_exec::context_operations_supported();
    async move {
        shim::errno(&env, async |frame| {
            let functions = GuestFunctionOperations::new(frame.env, frame.unwind())
                .with_context_support(contexts_supported);
            let caller = frame.caller().with_functions(&functions);
            let result = frame.syscalls.context_destroy(&caller, id as u64).await?;
            extension_result(result)
        })
        .await
    }
}

fn extension_result(value: i32) -> crate::errno::V1Result<()> {
    if value < 0 {
        let errno = value
            .checked_neg()
            .filter(|errno| *errno <= 4095)
            .unwrap_or(wasixv2_syscalls::Errno::EINVAL.0);
        return Err(wasixv2_abi::Errno(errno).into());
    }
    Ok(())
}
