use std::ptr::NonNull;
use wasmer_runtime_core::{
    typed_func::Trampoline,
    vm::{Ctx, Func},
};

type CallProtectedResult = Result<(), CallProtectedData>;

#[repr(C)]
pub struct CallProtectedData {
    pub code: u64,
    pub exception_address: u64,
    pub instruction_pointer: u64,
}

extern "C" {
    #[link_name = "callProtected"]
    pub fn __call_protected(
        trampoline: Trampoline,
        ctx: *mut Ctx,
        func: NonNull<Func>,
        param_vec: *const u64,
        return_vec: *mut u64,
        out_result: *mut CallProtectedData,
    ) -> u8;
}

pub fn _call_protected(
    trampoline: Trampoline,
    ctx: *mut Ctx,
    func: NonNull<Func>,
    param_vec: *const u64,
    return_vec: *mut u64,
) -> CallProtectedResult {
    let mut out_result = CallProtectedData {
        code: 0,
        exception_address: 0,
        instruction_pointer: 0,
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
