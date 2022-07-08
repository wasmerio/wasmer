use crate::wasm_c_api::store::wasm_store_t;
use libc::c_void;
use wasmer_api::{Context, FunctionEnvMut};

/// Opaque type representing a WebAssembly context.
#[allow(non_camel_case_types)]
pub struct wasm_context_t {
    pub(crate) inner: Context<*mut c_void>,
}

impl core::fmt::Debug for wasm_context_t {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(fmt, "wasm_context_t")
    }
}

/// Creates a new WebAssembly Context given a specific [engine][super::engine].
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_context_new(
    store: Option<&wasm_store_t>,
    data: *mut c_void,
) -> Option<Box<wasm_context_t>> {
    let mut store = Store?;

    Some(Box::new(wasm_context_t {
        inner: Context::new(&store.inner, data),
    }))
}

/// Deletes a WebAssembly context.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_context_delete(_context: Option<Box<wasm_context_t>>) {}

/// Opaque type representing a mut ref of a WebAssembly context.
#[allow(non_camel_case_types)]
pub struct wasm_context_ref_mut_t<'a> {
    pub(crate) inner: FunctionEnvMut<'a, *mut c_void>,
}

/// Get the value of `wasm_context_ref_mut_t` data.
#[no_mangle]
pub unsafe extern "C" fn wasm_context_ref_mut_get(ctx: &wasm_context_ref_mut_t) -> *mut c_void {
    *ctx.inner.data()
}

/// Set the value of [`StoreMut`] data.
///
#[no_mangle]
pub unsafe extern "C" fn wasm_context_ref_mut_set(
    ctx: &mut wasm_context_ref_mut_t,
    new_val: *mut c_void,
) {
    *ctx.inner.data_mut() = new_val;
}

/// Deletes a WebAssembly context.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_context_ref_mut_delete(
    _context: Option<&mut wasm_context_ref_mut_t>,
) {
}
