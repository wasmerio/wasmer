use super::store::wasm_store_t;
use super::types::{wasm_byte_vec_t, wasm_frame_t, wasm_frame_vec_t, wasm_message_t};
use wasmer::RuntimeError;

// opaque type which is a `RuntimeError`
/// cbindgen:ignore
#[allow(non_camel_case_types)]
pub struct wasm_trap_t {
    pub(crate) inner: RuntimeError,
}

impl From<RuntimeError> for wasm_trap_t {
    fn from(other: RuntimeError) -> Self {
        Self { inner: other }
    }
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_new(
    _store: &mut wasm_store_t,
    message: &wasm_message_t,
) -> Option<Box<wasm_trap_t>> {
    let message_bytes: &[u8] = message.into_slice()?;
    let message_str = c_try!(std::str::from_utf8(message_bytes));
    let runtime_error = RuntimeError::new(message_str);
    let trap = runtime_error.into();

    Some(Box::new(trap))
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_delete(_trap: Option<Box<wasm_trap_t>>) {}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_message(trap: &wasm_trap_t, out_ptr: &mut wasm_byte_vec_t) {
    let message = trap.inner.message();
    let byte_vec: wasm_byte_vec_t = message.into_bytes().into();
    out_ptr.size = byte_vec.size;
    out_ptr.data = byte_vec.data;
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_origin(trap: &wasm_trap_t) -> Option<Box<wasm_frame_t>> {
    trap.inner.trace().first().map(Into::into).map(Box::new)
}

/// cbindgen:ignore
#[no_mangle]
pub unsafe extern "C" fn wasm_trap_trace(trap: &wasm_trap_t, out_ptr: &mut wasm_frame_vec_t) {
    let frames = trap.inner.trace();
    let frame_vec: wasm_frame_vec_t = frames.into();

    out_ptr.size = frame_vec.size;
    out_ptr.data = frame_vec.data;
}
