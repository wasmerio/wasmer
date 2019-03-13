use std::ffi::c_void;
use wasmer_runtime_core::vm::{Ctx, Func};

type Trampoline = unsafe extern "C" fn(*mut Ctx, *const Func, *const u64, *mut u64) -> c_void;
type CallProtectedResult = Result<(), CallProtectedData>;

#[repr(C)]
pub struct CallProtectedData {
    pub code: u64,
    pub exceptionAddress: u64,
    pub instructionPointer: u64,
}

extern "C" {
    #[link_name = "callProtected"]
    pub fn __call_protected(
        trampoline: Trampoline,
        ctx: *mut Ctx,
        func: *const Func,
        param_vec: *const u64,
        return_vec: *mut u64,
        out_result: *mut CallProtectedData,
    ) -> u8;
}

pub fn _call_protected(
    trampoline: Trampoline,
    ctx: *mut Ctx,
    func: *const Func,
    param_vec: *const u64,
    return_vec: *mut u64,
) -> CallProtectedResult {
    let mut out_result = CallProtectedData {
        code: 0,
        exceptionAddress: 0,
        instructionPointer: 0,
    };
    let result = unsafe {
        __call_protected(
            trampoline,
            ctx,
            func,
            param_vec,
            return_vec,
            &mut out_result,
        )
    };
    if result == 1 {
        Ok(())
    } else {
        Err(out_result)
    }
}
