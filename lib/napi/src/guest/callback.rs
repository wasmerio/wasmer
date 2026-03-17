use std::ffi::c_void;

use wasmer::{FunctionEnvMut, Table, Value};

use crate::{RuntimeEnv, snapi::SnapiEnv};

use super::util::{write_guest_bytes, read_guest_bytes};

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

pub fn flush_staged_host_buffer_views_to_host(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    snapi_env: SnapiEnv,
) {
    if snapi_env.is_null() || env.data().host_buffer_views.is_empty() {
        return;
    }

    let staged_views = env
        .data()
        .host_buffer_views
        .iter()
        .map(|(key, view)| (*key, *view))
        .collect::<Vec<_>>();

    for (key, view) in staged_views {
        if view.byte_len > 0
            && let Some(bytes) = read_guest_bytes(env, view.guest_ptr as i32, view.byte_len)
        {
            unsafe {
                crate::snapi::snapi_bridge_overwrite_value_bytes(
                    snapi_env,
                    view.handle_id,
                    bytes.as_ptr().cast(),
                    view.byte_len as u32,
                );
            }
        }
        if let Some(current) = env.data_mut().host_buffer_views.get_mut(&key) {
            current.guest_dirty = false;
            current.handle_id = view.handle_id;
        }
    }
}

pub fn refresh_staged_host_buffer_views_from_host(
    env: &mut FunctionEnvMut<RuntimeEnv>,
    snapi_env: SnapiEnv,
) {
    if snapi_env.is_null() || env.data().host_buffer_views.is_empty() {
        return;
    }

    let staged_views = env
        .data()
        .host_buffer_views
        .iter()
        .map(|(key, view)| (*key, *view))
        .collect::<Vec<_>>();

    for (key, view) in staged_views {
        let mut snapshot_ptr = 0u64;
        let mut snapshot_len = 0u32;
        let status = unsafe {
            crate::snapi::snapi_bridge_snapshot_value_bytes(
                snapi_env,
                view.handle_id,
                &mut snapshot_ptr,
                &mut snapshot_len,
            )
        };
        if status != 0 {
            continue;
        }

        let copy_len = usize::min(snapshot_len as usize, view.byte_len);
        if copy_len > 0 {
            let snapshot = unsafe { std::slice::from_raw_parts(snapshot_ptr as *const u8, copy_len) };
            let _ = write_guest_bytes(env, view.guest_ptr, snapshot);
        }
        if copy_len < view.byte_len {
            let zeros = vec![0u8; view.byte_len - copy_len];
            let _ = write_guest_bytes(env, view.guest_ptr + copy_len as u32, &zeros);
        }
        if snapshot_ptr != 0 {
            unsafe {
                crate::snapi::snapi_bridge_unofficial_free_buffer(snapshot_ptr as *mut c_void);
            }
        }
        if let Some(current) = env.data_mut().host_buffer_views.get_mut(&key) {
            current.guest_dirty = false;
            current.handle_id = view.handle_id;
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

    flush_staged_host_buffer_views_to_host(env, snapi_env);
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
        env: *mut RawFunctionEnvMut,
    }
    impl Drop for CallbackStateGuard {
        fn drop(&mut self) {
            if !self.env.is_null() {
                let env = unsafe { &mut *self.env.cast::<FunctionEnvMut<'_, RuntimeEnv>>() };
                refresh_staged_host_buffer_views_from_host(env, self.snapi_env);
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
    let snapi_env = env.data().resolve_napi_env(guest_env as i32);
    refresh_staged_host_buffer_views_from_host(env, snapi_env);
    let result = call_guest_callback(env, &table, guest_env as i32, wasm_fn_ptr, callback_arg);
    flush_staged_host_buffer_views_to_host(env, snapi_env);
    result
}
