use crate::{
    relocation::{TrapCode, TrapData},
    signal::{CallProtError, HandlerData},
};
use std::{
    cell::Cell,
    ffi::c_void,
    ptr::{self, NonNull},
};
use wasmer_runtime_core::{
    typed_func::{Trampoline, WasmTrapInfo},
    vm::{Ctx, Func},
};
use wasmer_win_exception_handler::CallProtectedData;
pub use wasmer_win_exception_handler::_call_protected;
use winapi::{
    shared::minwindef::DWORD,
    um::minwinbase::{
        EXCEPTION_ACCESS_VIOLATION, EXCEPTION_ARRAY_BOUNDS_EXCEEDED, EXCEPTION_BREAKPOINT,
        EXCEPTION_DATATYPE_MISALIGNMENT, EXCEPTION_FLT_DENORMAL_OPERAND,
        EXCEPTION_FLT_DIVIDE_BY_ZERO, EXCEPTION_FLT_INEXACT_RESULT,
        EXCEPTION_FLT_INVALID_OPERATION, EXCEPTION_FLT_OVERFLOW, EXCEPTION_FLT_STACK_CHECK,
        EXCEPTION_FLT_UNDERFLOW, EXCEPTION_GUARD_PAGE, EXCEPTION_ILLEGAL_INSTRUCTION,
        EXCEPTION_INT_DIVIDE_BY_ZERO, EXCEPTION_INT_OVERFLOW, EXCEPTION_INVALID_HANDLE,
        EXCEPTION_IN_PAGE_ERROR, EXCEPTION_NONCONTINUABLE_EXCEPTION, EXCEPTION_POSSIBLE_DEADLOCK,
        EXCEPTION_PRIV_INSTRUCTION, EXCEPTION_SINGLE_STEP, EXCEPTION_STACK_OVERFLOW,
    },
};

thread_local! {
    pub static CURRENT_EXECUTABLE_BUFFER: Cell<*const c_void> = Cell::new(ptr::null());
}

pub fn call_protected(
    handler_data: &HandlerData,
    trampoline: Trampoline,
    ctx: *mut Ctx,
    func: NonNull<Func>,
    param_vec: *const u64,
    return_vec: *mut u64,
) -> Result<(), CallProtError> {
    // TODO: trap early
    // user code error
    //    if let Some(msg) = super::TRAP_EARLY_DATA.with(|cell| cell.replace(None)) {
    //        return Err(RuntimeError::User { msg });
    //    }

    let result = _call_protected(trampoline, ctx, func, param_vec, return_vec);

    if let Ok(_) = result {
        return Ok(());
    }

    let CallProtectedData {
        code,
        exception_address,
        instruction_pointer,
    } = result.unwrap_err();

    if let Some(TrapData {
        trapcode,
        srcloc: _,
    }) = handler_data.lookup(instruction_pointer as _)
    {
        Err(CallProtError::Trap(match code as DWORD {
            EXCEPTION_ACCESS_VIOLATION => WasmTrapInfo::MemoryOutOfBounds,
            EXCEPTION_ILLEGAL_INSTRUCTION => match trapcode {
                TrapCode::BadSignature => WasmTrapInfo::IncorrectCallIndirectSignature,
                TrapCode::IndirectCallToNull => WasmTrapInfo::CallIndirectOOB,
                TrapCode::HeapOutOfBounds => WasmTrapInfo::MemoryOutOfBounds,
                TrapCode::TableOutOfBounds => WasmTrapInfo::CallIndirectOOB,
                TrapCode::UnreachableCodeReached => WasmTrapInfo::Unreachable,
                _ => WasmTrapInfo::Unknown,
            },
            EXCEPTION_STACK_OVERFLOW => WasmTrapInfo::Unknown,
            EXCEPTION_INT_DIVIDE_BY_ZERO | EXCEPTION_INT_OVERFLOW => {
                WasmTrapInfo::IllegalArithmetic
            }
            _ => WasmTrapInfo::Unknown,
        }))
    } else {
        let signal = match code as DWORD {
            EXCEPTION_FLT_DENORMAL_OPERAND
            | EXCEPTION_FLT_DIVIDE_BY_ZERO
            | EXCEPTION_FLT_INEXACT_RESULT
            | EXCEPTION_FLT_INVALID_OPERATION
            | EXCEPTION_FLT_OVERFLOW
            | EXCEPTION_FLT_STACK_CHECK
            | EXCEPTION_FLT_UNDERFLOW => "floating-point exception",
            EXCEPTION_ILLEGAL_INSTRUCTION => "illegal instruction",
            EXCEPTION_ACCESS_VIOLATION => "segmentation violation",
            EXCEPTION_DATATYPE_MISALIGNMENT => "datatype misalignment",
            EXCEPTION_BREAKPOINT => "breakpoint",
            EXCEPTION_SINGLE_STEP => "single step",
            EXCEPTION_ARRAY_BOUNDS_EXCEEDED => "array bounds exceeded",
            EXCEPTION_INT_DIVIDE_BY_ZERO => "int div by zero",
            EXCEPTION_INT_OVERFLOW => "int overflow",
            EXCEPTION_PRIV_INSTRUCTION => "priv instruction",
            EXCEPTION_IN_PAGE_ERROR => "in page error",
            EXCEPTION_NONCONTINUABLE_EXCEPTION => "non continuable exception",
            EXCEPTION_STACK_OVERFLOW => "stack overflow",
            EXCEPTION_GUARD_PAGE => "guard page",
            EXCEPTION_INVALID_HANDLE => "invalid handle",
            EXCEPTION_POSSIBLE_DEADLOCK => "possible deadlock",
            _ => "unknown exception code",
        };

        let s = format!(
            "unhandled trap at {:x} - code #{:x}: {}",
            exception_address, code, signal,
        );

        Err(CallProtError::Error(Box::new(s)))
    }
}

pub unsafe fn trigger_trap() -> ! {
    // TODO
    unimplemented!("windows::trigger_trap");
}
