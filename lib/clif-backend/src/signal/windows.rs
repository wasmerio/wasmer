use crate::relocation::{TrapCode, TrapData};
use crate::signal::HandlerData;
use crate::trampoline::Trampoline;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use wasmer_runtime_core::vm::Ctx;
use wasmer_runtime_core::vm::Func;
use wasmer_runtime_core::{
    error::{RuntimeError, RuntimeResult},
    structures::TypedIndex,
    types::{MemoryIndex, TableIndex},
};
use wasmer_win_exception_handler::CallProtectedData;
pub use wasmer_win_exception_handler::_call_protected;
use winapi::shared::minwindef::DWORD;
use winapi::um::minwinbase::{
    EXCEPTION_ACCESS_VIOLATION, EXCEPTION_FLT_DENORMAL_OPERAND, EXCEPTION_FLT_DIVIDE_BY_ZERO,
    EXCEPTION_FLT_INEXACT_RESULT, EXCEPTION_FLT_INVALID_OPERATION, EXCEPTION_FLT_OVERFLOW,
    EXCEPTION_FLT_STACK_CHECK, EXCEPTION_FLT_UNDERFLOW, EXCEPTION_ILLEGAL_INSTRUCTION,
    EXCEPTION_INT_DIVIDE_BY_ZERO, EXCEPTION_INT_OVERFLOW, EXCEPTION_STACK_OVERFLOW,
};

thread_local! {
    pub static CURRENT_EXECUTABLE_BUFFER: Cell<*const c_void> = Cell::new(ptr::null());
}

pub fn call_protected(
    handler_data: &HandlerData,
    trampoline: Trampoline,
    ctx: *mut Ctx,
    func: *const Func,
    param_vec: *const u64,
    return_vec: *mut u64,
) -> RuntimeResult<()> {
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
        code: signum,
        exceptionAddress: exception_address,
        instructionPointer: instruction_pointer,
    } = result.unwrap_err();

    if let Some(TrapData {
        trapcode,
        srcloc: _,
    }) = handler_data.lookup(instruction_pointer as _)
    {
        Err(match signum as DWORD {
            EXCEPTION_ACCESS_VIOLATION => RuntimeError::OutOfBoundsAccess {
                memory: MemoryIndex::new(0),
                addr: None,
            },
            EXCEPTION_ILLEGAL_INSTRUCTION => match trapcode {
                TrapCode::BadSignature => RuntimeError::IndirectCallSignature {
                    table: TableIndex::new(0),
                },
                TrapCode::IndirectCallToNull => RuntimeError::IndirectCallToNull {
                    table: TableIndex::new(0),
                },
                TrapCode::HeapOutOfBounds => RuntimeError::OutOfBoundsAccess {
                    memory: MemoryIndex::new(0),
                    addr: None,
                },
                TrapCode::TableOutOfBounds => RuntimeError::TableOutOfBounds {
                    table: TableIndex::new(0),
                },
                _ => RuntimeError::Unknown {
                    msg: "unknown trap".to_string(),
                },
            },
            EXCEPTION_STACK_OVERFLOW => RuntimeError::Unknown {
                msg: "unknown trap".to_string(),
            },
            EXCEPTION_INT_DIVIDE_BY_ZERO => RuntimeError::IllegalArithmeticOperation,
            EXCEPTION_INT_OVERFLOW => RuntimeError::IllegalArithmeticOperation,
            _ => RuntimeError::Unknown {
                msg: "unknown trap".to_string(),
            },
        }
        .into())
    } else {
        let signal = match signum as DWORD {
            EXCEPTION_FLT_DENORMAL_OPERAND
            | EXCEPTION_FLT_DIVIDE_BY_ZERO
            | EXCEPTION_FLT_INEXACT_RESULT
            | EXCEPTION_FLT_INVALID_OPERATION
            | EXCEPTION_FLT_OVERFLOW
            | EXCEPTION_FLT_STACK_CHECK
            | EXCEPTION_FLT_UNDERFLOW => "floating-point exception",
            EXCEPTION_ILLEGAL_INSTRUCTION => "illegal instruction",
            EXCEPTION_ACCESS_VIOLATION => "segmentation violation",
            _ => "unkown trapped signal",
        };

        Err(RuntimeError::Unknown {
            msg: format!("trap at {} - {}", exception_address, signal),
        }
        .into())
    }
}

pub unsafe fn trigger_trap() -> ! {
    // TODO
    unimplemented!();
}
