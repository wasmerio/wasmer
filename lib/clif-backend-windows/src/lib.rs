use std::ffi::c_void;
use wasmer_runtime_core::vm::{Ctx, Func};

#[cfg(target_os = "windows")]
type func_t = Func;
#[cfg(target_os = "windows")]
type wasmer_instance_context_t = Ctx;
#[cfg(target_os = "windows")]
type Trampoline = unsafe extern "C" fn(*mut Ctx, *const func_t, *const u64, *mut u64) -> c_void
#[cfg(target_os = "windows")]
type CallProtectedResult = Result<(), CallProtectedData>;

#[cfg(target_os = "windows")]
#[repr(C)]
pub struct CallProtectedData {
    pub code: u64,
    pub exceptionAddress: u64,
    pub instructionPointer: u64,
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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
    println!("result from __call_protected: {}", result);
    if result == 1 {
        Ok(())
    } else {
        println!("returning error from _call_protected");
        Err(out_result)
    }
}
