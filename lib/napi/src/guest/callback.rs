use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

use wasmer::{AsStoreMut, FunctionEnvMut, StoreMut, Table, Value};

use crate::{snapi::SnapiEnv, RuntimeEnv};

// Thread-local raw pointer to the current FunctionEnvMut, used by the
// C++ → Rust callback trampoline. Set before any C++ FFI call that might
// trigger V8 callbacks, cleared after. Safe because:
// 1. Single-threaded WASM execution
// 2. Pointer is valid for the duration of the synchronous FFI call
// 3. Callback is strictly nested within the FFI call
thread_local! {
    pub static CB_ENV_PTR: Cell<*mut ()> = Cell::new(std::ptr::null_mut());
}

static CB_TOP_LEVEL: Mutex<Option<CallbackTopLevelState>> = Mutex::new(None);

#[derive(Clone)]
struct CallbackTopLevelState {
    // FIXME: remove this, this is terribly unsafe!!
    // Wasmer's StoreMut is a temporary wrapper around the underlying store
    // context. Persist the wrapped store pointer, not the StoreMut stack
    // object, otherwise later callbacks dereference freed stack memory.
    store_inner: *mut c_void,
    table: Table,
    guest_envs: HashMap<usize, u32>,
}

unsafe impl Send for CallbackTopLevelState {}

fn call_guest_callback(
    store: &mut impl wasmer::AsStoreMut,
    table: &Table,
    guest_env: i32,
    wasm_fn_ptr: u32,
    data_val: u64,
) -> u32 {
    let Some(elem) = table.get(store, wasm_fn_ptr) else {
        return 0;
    };
    let func = match elem {
        Value::FuncRef(Some(func)) => func,
        Value::FuncRef(None) => return 0,
        _ => return 0,
    };
    match func.call(store, &[Value::I32(guest_env), Value::I32(data_val as i32)]) {
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

pub fn set_top_level_callback_state(
    store: &mut StoreMut<'_>,
    table: Option<Table>,
    guest_envs: HashMap<usize, u32>,
) {
    let mut guard = CB_TOP_LEVEL
        .lock()
        .expect("callback top-level mutex poisoned");
    if let Some(table) = table {
        let raw: *mut c_void = unsafe { std::mem::transmute(store.as_store_mut()) };
        *guard = Some(CallbackTopLevelState {
            store_inner: raw,
            table,
            guest_envs,
        });
    } else {
        *guard = None;
    }
}

pub fn clear_top_level_callback_state() {
    let mut guard = CB_TOP_LEVEL
        .lock()
        .expect("callback top-level mutex poisoned");
    *guard = None;
}

/// Rust trampoline called from C++ when a V8 callback fires.
/// Retrieves the WASM store from the thread-local, then calls
/// __napi_callback_dispatch in the WASM guest.
#[no_mangle]
pub extern "C" fn snapi_host_invoke_wasm_callback(
    snapi_env: SnapiEnv,
    wasm_fn_ptr: u32,
    data_val: u64,
) -> u32 {
    CB_ENV_PTR.with(|cell| {
        let ptr = cell.get();
        if !ptr.is_null() {
            // SAFETY: ptr was set from &mut FunctionEnvMut<RuntimeEnv> which is still
            // alive on the call stack above us. Single-threaded, synchronous.
            let env: &mut FunctionEnvMut<'_, RuntimeEnv> =
                unsafe { &mut *(ptr as *mut FunctionEnvMut<'_, RuntimeEnv>) };
            let Some(table) = env.data().table.clone() else {
                return 0;
            };
            let Some(guest_env) = env.data().guest_napi_env(snapi_env) else {
                return 0;
            };
            let (_, mut store) = env.data_and_store_mut();
            return call_guest_callback(
                &mut store,
                &table,
                guest_env as i32,
                wasm_fn_ptr,
                data_val,
            );
        }

        let state = CB_TOP_LEVEL
            .lock()
            .expect("callback top-level mutex poisoned")
            .clone();
        if let Some(state) = state {
            let mut store: StoreMut<'_> = unsafe { std::mem::transmute(state.store_inner) };
            let guest_env = state
                .guest_envs
                .get(&(snapi_env as usize))
                .copied()
                .unwrap_or(0);
            return call_guest_callback(
                &mut store,
                &state.table,
                guest_env as i32,
                wasm_fn_ptr,
                data_val,
            );
        }
        eprintln!("[callback trampoline] no env pointer available");
        0
    })
}
