//! Trampoline emitter for transforming function calls.

use std::ffi::c_void;
use std::mem;
use wasmer_runtime_core::trampoline::*;

#[repr(C)]
pub struct wasmer_trampoline_buffer_builder_t;

#[repr(C)]
pub struct wasmer_trampoline_buffer_t;

#[repr(C)]
pub struct wasmer_trampoline_callable_t;

/// Creates a new trampoline builder.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn wasmer_trampoline_buffer_builder_new() -> *mut wasmer_trampoline_buffer_builder_t
{
    Box::into_raw(Box::new(TrampolineBufferBuilder::new())) as *mut _
}

/// Adds a context trampoline to the builder.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_builder_add_context_trampoline(
    builder: *mut wasmer_trampoline_buffer_builder_t,
    func: *const wasmer_trampoline_callable_t,
    ctx: *const c_void,
) -> usize {
    let builder = &mut *(builder as *mut TrampolineBufferBuilder);
    builder.add_context_trampoline(func as *const CallTarget, ctx as *const CallContext)
}

/// Adds a callinfo trampoline to the builder.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_builder_add_callinfo_trampoline(
    builder: *mut wasmer_trampoline_buffer_builder_t,
    func: *const wasmer_trampoline_callable_t,
    ctx: *const c_void,
    num_params: u32,
) -> usize {
    let builder = &mut *(builder as *mut TrampolineBufferBuilder);
    builder.add_callinfo_trampoline(mem::transmute(func), ctx as *const CallContext, num_params)
}

/// Finalizes the trampoline builder into an executable buffer.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_builder_build(
    builder: *mut wasmer_trampoline_buffer_builder_t,
) -> *mut wasmer_trampoline_buffer_t {
    let builder = Box::from_raw(builder as *mut TrampolineBufferBuilder);
    Box::into_raw(Box::new(builder.build())) as *mut _
}

/// Destroys the trampoline buffer if not null.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_destroy(buffer: *mut wasmer_trampoline_buffer_t) {
    if !buffer.is_null() {
        Box::from_raw(buffer as *mut TrampolineBuffer);
    }
}

/// Returns the callable pointer for the trampoline with index `idx`.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_get_trampoline(
    buffer: *const wasmer_trampoline_buffer_t,
    idx: usize,
) -> *const wasmer_trampoline_callable_t {
    let buffer = &*(buffer as *const TrampolineBuffer);
    buffer.get_trampoline(idx) as _
}

/// Returns the context added by `add_context_trampoline`, from within the callee function.
#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_get_context() -> *mut c_void {
    get_context() as *const c_void as *mut c_void
}
