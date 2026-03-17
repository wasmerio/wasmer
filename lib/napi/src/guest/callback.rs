use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    sync::{LazyLock, Mutex},
};

use wasmer::{AsStoreMut, FunctionEnvMut, StoreMut, Table, Value};

use crate::{RuntimeEnv, snapi::SnapiEnv};

#[derive(Clone)]
struct CallbackTopLevelState {
    store_inner: *mut c_void,
    runtime_env: *mut RuntimeEnv,
    table: Table,
}

unsafe impl Send for CallbackTopLevelState {}

static CB_TOP_LEVEL: LazyLock<Mutex<HashMap<usize, CallbackTopLevelState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn call_guest_callback(
    store: &mut impl AsStoreMut,
    table: &Table,
    guest_env: i32,
    wasm_fn_ptr: u32,
    callback_arg: u32,
) -> u32 {
    let Some(elem) = table.get(store, wasm_fn_ptr) else {
        return 0;
    };
    let func = match elem {
        Value::FuncRef(Some(func)) => func,
        Value::FuncRef(None) => return 0,
        _ => return 0,
    };
    match func.call(
        store,
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

fn read_guest_bytes_from_state(
    runtime_env: &RuntimeEnv,
    store: &mut StoreMut<'_>,
    guest_ptr: u32,
    len: usize,
) -> Option<Vec<u8>> {
    let memory = runtime_env.memory.clone()?;
    let view = memory.view(&mut *store);
    let mut out = vec![0u8; len];
    view.read(guest_ptr as u64, &mut out).ok()?;
    Some(out)
}

fn flush_host_buffer_copies_since(
    runtime_env: &mut RuntimeEnv,
    store: &mut StoreMut<'_>,
    snapi_env: SnapiEnv,
    frame_start: usize,
) {
    let start = frame_start.min(runtime_env.host_buffer_copies.len());
    let drained = { runtime_env.host_buffer_copies.split_off(start) };

    for mapping in drained {
        if mapping.byte_len > 0
            && let Some(bytes) =
                read_guest_bytes_from_state(runtime_env, store, mapping.guest_ptr, mapping.byte_len)
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

        runtime_env.guest_data_ptrs.remove(&mapping.handle_id);
        if mapping.backing_store_token != 0 {
            runtime_env
                .guest_data_backing_stores
                .remove(&mapping.backing_store_token);
        }
    }
}

pub fn set_top_level_callback_state(env: &mut FunctionEnvMut<RuntimeEnv>, snapi_env: SnapiEnv) {
    if snapi_env.is_null() {
        return;
    }

    let Some(table) = env.data().table.clone() else {
        CB_TOP_LEVEL
            .lock()
            .expect("callback top-level mutex poisoned")
            .remove(&(snapi_env as usize));
        return;
    };

    let (runtime_env, mut store) = env.data_and_store_mut();
    runtime_env
        .callback_env_keys
        .lock()
        .expect("callback env key mutex poisoned")
        .insert(snapi_env as usize);

    let store_inner: *mut c_void = unsafe { std::mem::transmute(store.as_store_mut()) };
    CB_TOP_LEVEL
        .lock()
        .expect("callback top-level mutex poisoned")
        .insert(
            snapi_env as usize,
            CallbackTopLevelState {
                store_inner,
                runtime_env: runtime_env as *mut RuntimeEnv,
                table,
            },
        );
}

pub fn clear_top_level_callback_state(env: &mut FunctionEnvMut<RuntimeEnv>, snapi_env: SnapiEnv) {
    if snapi_env.is_null() {
        return;
    }
    env.data()
        .callback_env_keys
        .lock()
        .expect("callback env key mutex poisoned")
        .remove(&(snapi_env as usize));
    CB_TOP_LEVEL
        .lock()
        .expect("callback top-level mutex poisoned")
        .remove(&(snapi_env as usize));
}

pub fn clear_tracked_top_level_callback_states(callback_env_keys: &Mutex<HashSet<usize>>) {
    let keys = {
        let mut guard = callback_env_keys
            .lock()
            .expect("callback env key mutex poisoned");
        std::mem::take(&mut *guard)
    };

    let mut guard = CB_TOP_LEVEL
        .lock()
        .expect("callback top-level mutex poisoned");
    for key in keys {
        guard.remove(&key);
    }
}

pub fn with_callback_state<R>(
    _env: &mut FunctionEnvMut<RuntimeEnv>,
    _snapi_env: SnapiEnv,
    f: impl FnOnce() -> R,
) -> R {
    f()
}

/// Rust trampoline called from C++ when a V8 callback fires.
/// Re-enters the guest callback using stable per-env runtime state.
#[unsafe(no_mangle)]
pub extern "C" fn snapi_host_invoke_wasm_callback(
    snapi_env: SnapiEnv,
    guest_env: u32,
    wasm_fn_ptr: u32,
    callback_arg: u32,
) -> u32 {
    let Some(state) = CB_TOP_LEVEL
        .lock()
        .expect("callback top-level mutex poisoned")
        .get(&(snapi_env as usize))
        .cloned()
    else {
        eprintln!("[callback trampoline] no top-level callback state available");
        return 0;
    };

    let mut store: StoreMut<'_> = unsafe { std::mem::transmute(state.store_inner) };
    let runtime_env = unsafe { &mut *state.runtime_env };

    let frame_start = runtime_env.host_buffer_copies.len();
    runtime_env.host_buffer_copy_frames.push(frame_start);

    struct CallbackFrameGuard {
        snapi_env: SnapiEnv,
        store_inner: *mut c_void,
        runtime_env: *mut RuntimeEnv,
        frame_start: usize,
    }

    impl Drop for CallbackFrameGuard {
        fn drop(&mut self) {
            let mut store: StoreMut<'_> = unsafe { std::mem::transmute(self.store_inner) };
            let runtime_env = unsafe { &mut *self.runtime_env };
            flush_host_buffer_copies_since(
                runtime_env,
                &mut store,
                self.snapi_env,
                self.frame_start,
            );
            runtime_env.host_buffer_copy_frames.pop();
        }
    }

    let _guard = CallbackFrameGuard {
        snapi_env,
        store_inner: state.store_inner,
        runtime_env: state.runtime_env,
        frame_start,
    };

    call_guest_callback(
        &mut store,
        &state.table,
        guest_env as i32,
        wasm_fn_ptr,
        callback_arg,
    )
}
