//! Entrypoints for the standard C API

#[macro_use]
pub mod macros;

/// cbindgen:ignore
pub mod engine;

/// cbindgen:ignore
pub mod externals;

/// cbindgen:ignore
pub mod instance;

/// cbindgen:ignore
pub mod module;

/// cbindgen:ignore
pub mod store;

/// cbindgen:ignore
pub mod trap;

/// cbindgen:ignore
pub mod types;

/// cbindgen:ignore
pub mod value;

#[cfg(feature = "wasi")]
pub mod wasi;

// TODO: find a home for this function
#[no_mangle]
pub unsafe extern "C" fn wasm_instance_get_vmctx_ptr(instance: &wasm_instance_t) -> *mut c_void {
    instance.inner.vmctx_ptr() as _
}
