use std::ffi::c_void;

use wasmer::{FunctionEnvMut, Table, Value};

use crate::{RuntimeEnv, snapi::SnapiEnv};

type RawFunctionEnvMut = FunctionEnvMut<'static, RuntimeEnv>;

#[repr(C)]
struct CallbackInvocationCtx {
    env: *mut RawFunctionEnvMut,
}

fn call_guest_callback(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    table: &Table,
    guest_env: i32,
    wasm_fn_ptr: u32,
    callback_arg: u32,
) -> u32 {
    let Some(elem) = table.get(env, wasm_fn_ptr) else {
        return 0;
    };
    let func = match elem {
        Value::FuncRef(Some(func)) => func,
        Value::FuncRef(None) => return 0,
        _ => return 0,
    };
    match func.call(
        env,
        &[Value::I32(guest_env), Value::I32(callback_arg as i32)],
    ) {
        Ok(ret_vals) => match ret_vals.first() {
            Some(Value::I32(v)) => *v as u32,
            Some(Value::I64(v)) => *v as u32,
            _ => 0,
        },
        Err(err) => {
            eprintln!("[callback trampoline] error calling function: {err}");
            0
        }
    }
}

pub fn with_callback_state<R>(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    snapi_env: SnapiEnv,
    f: impl FnOnce() -> R,
) -> R {
    if snapi_env.is_null() {
        return f();
    }

    let mut ctx = CallbackInvocationCtx {
        env: (env as *mut FunctionEnvMut<'_, RuntimeEnv>).cast::<RawFunctionEnvMut>(),
    };
    let prev = unsafe {
        crate::snapi::snapi_bridge_swap_active_callback_ctx(
            snapi_env,
            (&mut ctx as *mut CallbackInvocationCtx).cast::<c_void>(),
        )
    };
    struct CallbackStateGuard {
        snapi_env: SnapiEnv,
        prev: *mut c_void,
    }
    impl Drop for CallbackStateGuard {
        fn drop(&mut self) {
            unsafe {
                crate::snapi::snapi_bridge_swap_active_callback_ctx(self.snapi_env, self.prev);
            }
        }
    }
    let _guard = CallbackStateGuard { snapi_env, prev };
    f()
}

/// Rust trampoline called from C++ when a V8 callback fires.
/// Re-enters the active guest callback scope and dispatches into the WASM guest.
#[unsafe(no_mangle)]
pub extern "C" fn snapi_host_invoke_wasm_callback(
    callback_ctx: *mut c_void,
    guest_env: u32,
    wasm_fn_ptr: u32,
    callback_arg: u32,
) -> u32 {
    if callback_ctx.is_null() {
        eprintln!("[callback trampoline] no active callback scope available");
        return 0;
    }
    let ctx = unsafe { &mut *(callback_ctx as *mut CallbackInvocationCtx) };
    if ctx.env.is_null() {
        eprintln!("[callback trampoline] callback scope env cleared");
        return 0;
    }
    let env = unsafe { &mut *ctx.env.cast::<FunctionEnvMut<'_, RuntimeEnv>>() };
    let Some(table) = env.data().table.clone() else {
        return 0;
    };
    call_guest_callback(env, &table, guest_env as i32, wasm_fn_ptr, callback_arg)
}
