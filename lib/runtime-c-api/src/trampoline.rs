use std::ffi::c_void;
use wasmer_runtime_core::trampoline::*;

#[repr(C)]
pub struct wasmer_trampoline_buffer_builder_t;

#[repr(C)]
pub struct wasmer_trampoline_buffer_t;

#[repr(C)]
pub struct wasmer_trampoline_callable_t;

#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub extern "C" fn wasmer_trampoline_buffer_builder_new() -> *mut wasmer_trampoline_buffer_builder_t
{
    Box::into_raw(Box::new(TrampolineBufferBuilder::new())) as *mut _
}

#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_builder_add_context_trampoline(
    b: *mut wasmer_trampoline_buffer_builder_t,
    f: *const wasmer_trampoline_callable_t,
    ctx: *const c_void,
) -> usize {
    let b = &mut *(b as *mut TrampolineBufferBuilder);
    b.add_context_trampoline(f as *const CallTarget, ctx as *const CallContext)
}

#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_builder_add_callinfo_trampoline(
    b: *mut wasmer_trampoline_buffer_builder_t,
    f: *const wasmer_trampoline_callable_t,
    ctx: *const c_void,
    num_params: u32,
) -> usize {
    let b = &mut *(b as *mut TrampolineBufferBuilder);
    b.add_callinfo_trampoline(::std::mem::transmute(f), ctx as *const CallContext, num_params)
}

#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_builder_build(
    b: *mut wasmer_trampoline_buffer_builder_t,
) -> *mut wasmer_trampoline_buffer_t {
    let b = Box::from_raw(b as *mut TrampolineBufferBuilder);
    Box::into_raw(Box::new(b.build())) as *mut _
}

#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_destroy(b: *mut wasmer_trampoline_buffer_t) {
    Box::from_raw(b);
}

#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_buffer_get_trampoline(
    b: *const wasmer_trampoline_buffer_t,
    idx: usize,
) -> *const wasmer_trampoline_callable_t {
    let b = &*(b as *const TrampolineBuffer);
    b.get_trampoline(idx) as _
}

#[no_mangle]
#[allow(clippy::cast_ptr_alignment)]
pub unsafe extern "C" fn wasmer_trampoline_get_context() -> *mut c_void {
    get_context() as *const c_void as *mut c_void
}
