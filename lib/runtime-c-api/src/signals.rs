use std::sync::atomic::Ordering;
use wasmer_runtime_core::fault::{install_sighandler_as_dylib, SIGSEGV_PASSTHROUGH};

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_set_sigsegv_passthrough() {
    SIGSEGV_PASSTHROUGH.swap(true, Ordering::SeqCst);
}

#[allow(clippy::cast_ptr_alignment)]
#[no_mangle]
pub unsafe extern "C" fn wasmer_force_install_sighandlers() {
    install_sighandler_as_dylib();
}
