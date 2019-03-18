#include <windows.h>
#include <setjmp.h>
#include "exception_handling.h"

#define CALL_FIRST 1

__declspec(thread) jmp_buf jmpBuf;
__declspec(thread) PVOID caughtExceptionAddress;
__declspec(thread) DWORD64 caughtInstructionPointer;
__declspec(thread) PVOID savedStackPointer;
__declspec(thread) BOOL exceptionHandlerInstalled = FALSE;
__declspec(thread) BOOL alreadyHandlingException = FALSE;
__declspec(thread) PVOID handle;

void longjmpOutOfHere() {
    longjmp(jmpBuf, 1);
}

/// Get the current address that we use to jmp, the no inline is important
static __declspec(noinline) void *get_callee_frame_address(void) {
    return _AddressOfReturnAddress();
}

static LONG WINAPI
exceptionHandler(struct _EXCEPTION_POINTERS *ExceptionInfo) {
    EXCEPTION_RECORD* pExceptionRecord = ExceptionInfo->ExceptionRecord;
    PCONTEXT pCONTEXT = ExceptionInfo->ContextRecord;
    caughtExceptionAddress = pExceptionRecord->ExceptionAddress;
    caughtInstructionPointer = pCONTEXT->Rip;
    if (alreadyHandlingException == TRUE) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    alreadyHandlingException = TRUE;

    // Basically, here, we coerce the os to resume us into a context that calls `longjmp` instead of just continuing.
    // Presumably, we cannot `longjmp` out of the signal/exception context, like we can on unix.
    pCONTEXT->Rip = (uintptr_t)(&longjmpOutOfHere);
    pCONTEXT->Rsp = (uintptr_t)(savedStackPointer);
    return EXCEPTION_CONTINUE_EXECUTION;
}

static void removeExceptionHandler() {
    if (exceptionHandlerInstalled == FALSE) {
        return;
    }
    RemoveVectoredExceptionHandler(handle);
    exceptionHandlerInstalled = FALSE;
}

uint8_t callProtected(trampoline_t trampoline,
        const struct wasmer_instance_context_t* ctx,
        const struct func_t* func,
        const uint64_t* param_vec,
        uint64_t* return_vec,
        struct call_protected_result_t* out_result) {

    // install exception handler
    if (exceptionHandlerInstalled == FALSE) {
        exceptionHandlerInstalled = TRUE;
        handle = AddVectoredExceptionHandler(CALL_FIRST, exceptionHandler);
    }

    // jmp jmp jmp!
    int signum = setjmp(jmpBuf);
    if (signum == 0) {
        // save the stack pointer
        savedStackPointer = get_callee_frame_address();
        trampoline(ctx, func, param_vec, return_vec);
        out_result->code = 0;
        out_result->exception_address = 0;
        out_result->instruction_pointer = 0;

        removeExceptionHandler();
        return TRUE;
    }

    out_result->code = (uint64_t)signum;
    out_result->exception_address = (uint64_t)caughtExceptionAddress;
    out_result->instruction_pointer = caughtInstructionPointer;

    caughtExceptionAddress = 0;
    caughtInstructionPointer = 0;

    removeExceptionHandler();
    return FALSE;
}
