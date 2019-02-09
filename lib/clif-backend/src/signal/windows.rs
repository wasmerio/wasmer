use crate::relocation::{TrapCode, TrapData};
use crate::signal::HandlerData;
use libc::c_int;
type c_void = libc::c_void;
use std::cell::{Cell, UnsafeCell};
use std::ptr;
use std::sync::Once;
use wasmer_runtime_core::{
    error::{RuntimeError, RuntimeResult},
    structures::TypedIndex,
    types::{MemoryIndex, TableIndex},
};
use winapi::shared::minwindef::{DWORD, ULONG};
use winapi::um::errhandlingapi::AddVectoredExceptionHandler;
use winapi::um::minwinbase::{
    EXCEPTION_ACCESS_VIOLATION, EXCEPTION_FLT_DENORMAL_OPERAND, EXCEPTION_FLT_DIVIDE_BY_ZERO,
    EXCEPTION_FLT_INEXACT_RESULT, EXCEPTION_FLT_INVALID_OPERATION, EXCEPTION_FLT_OVERFLOW,
    EXCEPTION_FLT_STACK_CHECK, EXCEPTION_FLT_UNDERFLOW, EXCEPTION_ILLEGAL_INSTRUCTION,
    EXCEPTION_INT_DIVIDE_BY_ZERO, EXCEPTION_INT_OVERFLOW, EXCEPTION_STACK_OVERFLOW,
};
use winapi::um::winnt::{CONTEXT, EXCEPTION_POINTERS, EXCEPTION_RECORD};

extern "C" {
    pub fn setjmp(env: *mut c_void) -> c_int;
    fn longjmp(env: *mut c_void, val: c_int) -> !;
}

const SETJMP_BUFFER_LEN: usize = 27;
pub static SIGHANDLER_INIT: Once = Once::new();

thread_local! {
    pub static SETJMP_BUFFER: UnsafeCell<[c_int; SETJMP_BUFFER_LEN]> = UnsafeCell::new([0; SETJMP_BUFFER_LEN]);
    pub static CAUGHT_ADDRESSES: Cell<(*const c_void, *const c_void)> = Cell::new((ptr::null(), ptr::null()));
    pub static CURRENT_EXECUTABLE_BUFFER: Cell<*const c_void> = Cell::new(ptr::null());
}

unsafe extern "system" fn exception_handler(exception: *mut EXCEPTION_POINTERS) -> i32 {
    let exception_record = (*exception).ExceptionRecord;
    let exception_code = (*exception_record).ExceptionCode;
    let exception_record = (*exception_record).ExceptionRecord;
    let exception_context_record = (*exception).ContextRecord;
    do_unwind(exception_code, exception_record, exception_context_record)
}

/// Unwinds to last protected_call.
pub unsafe fn do_unwind(
    exception_code: DWORD,
    exception_record_ptr: *const EXCEPTION_RECORD,
    context_ptr: *const CONTEXT,
) -> ! {
    // Since do_unwind is only expected to get called from WebAssembly code which doesn't hold any host resources (locks etc.)
    // itself, accessing TLS here is safe. In case any other code calls this, it often indicates a memory safety bug and you should
    // temporarily disable the signal handlers to debug it.

    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    if *jmp_buf == [0; SETJMP_BUFFER_LEN] {
        ::std::process::abort();
    }

    let exception_address = (*exception_record_ptr).ExceptionAddress;
    let instruction_pointer = (*context_ptr).Rip;

    CAUGHT_ADDRESSES.with(|cell| cell.set((exception_address, instruction_pointer as _)));

    longjmp(jmp_buf as *mut c_void, exception_code as c_int)
}

pub fn call_protected<T>(handler_data: &HandlerData, f: impl FnOnce() -> T) -> RuntimeResult<T> {
    unsafe {
        let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
        let prev_jmp_buf = *jmp_buf;

        SIGHANDLER_INIT.call_once(|| {
            install_exception_handler();
        });

        let signum = setjmp(jmp_buf as *mut _);

        if signum == 0 {
            let ret = f(); // TODO: Switch stack?
            *jmp_buf = prev_jmp_buf;
            return Ok(ret);
        }
        // else signum != 0

        *jmp_buf = prev_jmp_buf;

        // user code error
        if let Some(msg) = super::TRAP_EARLY_DATA.with(|cell| cell.replace(None)) {
            return Err(RuntimeError::User { msg });
        }

        let (faulting_addr, inst_ptr) = CAUGHT_ADDRESSES.with(|cell| cell.get());

        if let Some(TrapData {
            trapcode,
            srcloc: _,
        }) = handler_data.lookup(inst_ptr)
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
            //             When the trap-handler is fully implemented, this will return more information.
            Err(RuntimeError::Unknown {
                msg: format!("trap at {:p} - {}", faulting_addr, signal),
            }
            .into())
        }
    }
}

pub unsafe fn trigger_trap() -> ! {
    let jmp_buf = SETJMP_BUFFER.with(|buf| buf.get());
    longjmp(jmp_buf as *mut c_void, 0)
}

unsafe fn install_exception_handler() {
    let first_handler: ULONG = 1; // true
    let exception_handler_option = Option::Some(exception_handler as _);
    AddVectoredExceptionHandler(first_handler, exception_handler_option);
}
