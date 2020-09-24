use super::types::wasm_byte_vec_t;
use std::mem;
use std::ptr::NonNull;
use wasmer::RuntimeError;

// opaque type which is a `RuntimeError`
#[repr(C)]
pub struct wasm_trap_t {}

#[no_mangle]
pub unsafe extern "C" fn wasm_trap_delete(trap: Option<NonNull<wasm_trap_t>>) {
    if let Some(t_inner) = trap {
        let _ = Box::from_raw(t_inner.cast::<RuntimeError>().as_ptr());
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_trap_message(
    trap: *const wasm_trap_t,
    out_ptr: *mut wasm_byte_vec_t,
) {
    let re = &*(trap as *const RuntimeError);
    // this code assumes no nul bytes appear in the message
    let mut message = format!("{}\0", re);
    message.shrink_to_fit();

    // TODO use `String::into_raw_parts` when it gets stabilized
    (*out_ptr).size = message.as_bytes().len();
    (*out_ptr).data = message.as_mut_ptr();
    mem::forget(message);
}

// in trap/RuntimeError we need to store
// 1. message
// 2. origin (frame); frame contains:
//    1. func index
//    2. func offset
//    3. module offset
//    4. which instance this was apart of

/*#[no_mangle]
pub unsafe extern "C" fn wasm_trap_trace(trap: *const wasm_trap_t, out_ptr: *mut wasm_frame_vec_t) {
    let re = &*(trap as *const RuntimeError);
    todo!()
}*/

//wasm_declare_ref!(trap);
//wasm_declare_ref!(foreign);
