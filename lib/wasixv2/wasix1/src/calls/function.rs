//! V1 function imports select their existing buffer conventions before layered dispatch.

use crate::abi::{GuestSize, Pointer};

use crate::shim::{self, Env};
use wasixv2_abi::CallDynamicFlags;
use wasixv2_exec::{CallBufferPolicy, GuestFunctionOperations};
use wasixv2_syscalls::{Control, RawGuestBuffer, SysErr, SysResult, Syscalls};
use wasmer::RuntimeError;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn call_dynamic<S: Syscalls + Control + 'static>(
    env: Env<S>,
    function: i32,
    values: Pointer,
    values_len: GuestSize,
    results: Pointer,
    results_len: GuestSize,
    strict: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let functions = GuestFunctionOperations::new(frame.env, frame.unwind()).with_buffer_policy(
            CallBufferPolicy {
                strict_results_before_call: false,
                zero_result_tail: false,
                whole_result_fields: true,
                validate_empty_reflection_spans: true,
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
                RawGuestBuffer::new(values.0, values_len.0),
                RawGuestBuffer::new(results.0, results_len.0),
                flags,
            )
            .await;
        let result = function_result(&functions, result)?;
        extension_result(result)
    })
    .await
}

pub(crate) async fn reflect_signature<S: Syscalls + Control + 'static>(
    env: Env<S>,
    function: i32,
    arguments: Pointer,
    arguments_len: i32,
    results: Pointer,
    results_len: i32,
    result: Pointer,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let functions = GuestFunctionOperations::new(frame.env, frame.unwind()).with_buffer_policy(
            CallBufferPolicy {
                validate_empty_reflection_spans: true,
                ..CallBufferPolicy::default()
            },
        );
        let caller = frame.caller().with_functions(&functions);
        let result = frame
            .syscalls
            .reflect_signature(
                &caller,
                function as u32,
                RawGuestBuffer::new(arguments.0, u64::from(arguments_len as u16)),
                RawGuestBuffer::new(results.0, u64::from(results_len as u16)),
                frame.raw_pointer(result),
            )
            .await;
        let result = function_result(&functions, result)?;
        extension_result(result)
    })
    .await
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

fn function_result<S: 'static>(
    functions: &GuestFunctionOperations<'_, S>,
    result: SysResult<i32>,
) -> crate::errno::V1Result<i32> {
    let faulted = functions.take_buffer_fault();
    match result {
        Err(SysErr::Errno(wasixv2_abi::Errno::EFAULT)) if faulted => {
            Err(crate::abi::Errno::MEMVIOLATION.into())
        }
        result => result.map_err(Into::into),
    }
}
