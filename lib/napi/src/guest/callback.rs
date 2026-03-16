use std::ffi::c_void;

use wasmer::{FunctionEnvMut, Table, Value};

use crate::{RuntimeEnv, snapi::SnapiEnv};

use super::util::read_guest_bytes;

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

fn flush_host_buffer_copies(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    snapi_env: SnapiEnv,
    frame_start: usize,
) {
    flush_host_buffer_copies_since(env, snapi_env, frame_start);
    env.data_mut().host_buffer_copy_frames.pop();
}

pub fn flush_pending_host_buffer_copies(env: &mut FunctionEnvMut<RuntimeEnv>, snapi_env: SnapiEnv) {
    if snapi_env.is_null() || env.data().host_buffer_copies.is_empty() {
        return;
    }

    let drained = {
        let state = env.data_mut();
        state
            .host_buffer_copy_frames
            .iter_mut()
            .for_each(|start| *start = 0);
        std::mem::take(&mut state.host_buffer_copies)
    };

    for mapping in drained {
        if mapping.byte_len > 0
            && let Some(bytes) = read_guest_bytes(env, mapping.guest_ptr as i32, mapping.byte_len)
        {
            unsafe {
                crate::snapi::snapi_bridge_overwrite_value_bytes(
                    snapi_env,
                    mapping.handle_id,
                    bytes.as_ptr().cast(),
                    mapping.byte_len as u32,
                );
            }
        }

        let state = env.data_mut();
        state.guest_data_ptrs.remove(&mapping.handle_id);
        if mapping.backing_store_token != 0 {
            state
                .guest_data_backing_stores
                .remove(&mapping.backing_store_token);
        }
    }
}

pub fn flush_host_buffer_copies_since(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    snapi_env: SnapiEnv,
    frame_start: usize,
) {
    let start = frame_start.min(env.data().host_buffer_copies.len());
    let drained = {
        let state = env.data_mut();
        state.host_buffer_copies.split_off(start)
    };

    for mapping in drained {
        if mapping.byte_len > 0
            && let Some(bytes) = read_guest_bytes(env, mapping.guest_ptr as i32, mapping.byte_len)
        {
            unsafe {
                crate::snapi::snapi_bridge_overwrite_value_bytes(
                    snapi_env,
                    mapping.handle_id,
                    bytes.as_ptr().cast(),
                    mapping.byte_len as u32,
                );
            }
        }

        let state = env.data_mut();
        state.guest_data_ptrs.remove(&mapping.handle_id);
        if mapping.backing_store_token != 0 {
            state
                .guest_data_backing_stores
                .remove(&mapping.backing_store_token);
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
    let frame_start = env.data().host_buffer_copies.len();
    let method_frame_depth = env.data().host_buffer_method_frames.len();
    env.data_mut().host_buffer_copy_frames.push(frame_start);
    let prev = unsafe {
        crate::snapi::snapi_bridge_swap_active_callback_ctx(
            snapi_env,
            (&mut ctx as *mut CallbackInvocationCtx).cast::<c_void>(),
        )
    };
    struct CallbackStateGuard {
        snapi_env: SnapiEnv,
        prev: *mut c_void,
        env: *mut RawFunctionEnvMut,
        frame_start: usize,
        method_frame_depth: usize,
    }
    impl Drop for CallbackStateGuard {
        fn drop(&mut self) {
            if !self.env.is_null() {
                let env = unsafe { &mut *self.env.cast::<FunctionEnvMut<'_, RuntimeEnv>>() };
                flush_host_buffer_copies(env, self.snapi_env, self.frame_start);
                env.data_mut()
                    .host_buffer_method_frames
                    .truncate(self.method_frame_depth);
                if self.frame_start > 0 {
                    flush_pending_host_buffer_copies(env, self.snapi_env);
                }
            }
            unsafe {
                crate::snapi::snapi_bridge_swap_active_callback_ctx(self.snapi_env, self.prev);
            }
        }
    }
    let _guard = CallbackStateGuard {
        snapi_env,
        prev,
        env: ctx.env,
        frame_start,
        method_frame_depth,
    };
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
