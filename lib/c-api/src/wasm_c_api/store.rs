use super::context::wasm_context_t;
use super::engine::wasm_engine_t;
use libc::c_void;
use std::cell::RefCell;
use std::rc::Rc;
use wasmer_api::Store;

/// Opaque type representing a WebAssembly store.
#[allow(non_camel_case_types)]
pub struct wasm_store_t {
    pub(crate) inner: Store,
    pub(crate) context: Rc<RefCell<wasm_context_t>>,
}

/// Creates a new WebAssembly store given a specific [engine][super::engine].
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_new(
    engine: Option<&wasm_engine_t>,
) -> Option<Box<wasm_store_t>> {
    let engine = engine?;
    let store = Store::new_with_engine(&*engine.inner);
    // Default context.
    let context = Rc::new(RefCell::new(wasm_context_t {
        inner: wasmer_api::Context::new(&store, std::ptr::null_mut()),
    }));

    Some(Box::new(wasm_store_t {
        inner: store,
        context,
    }))
}

/// Sets the context for this WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_context_set(
    store: &mut wasm_store_t,
    context: Box<wasm_context_t>,
) {
    store.context = Rc::new(RefCell::new(*context));
}

/// Get the value of Context data.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_data_get(store: &wasm_store_t) -> *mut c_void {
    *store.context.borrow().inner.data()
}

/// Set the value of Context data.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_data_set(store: &mut wasm_store_t, new_val: *mut c_void) {
    *store.context.borrow_mut().inner.data_mut() = new_val;
}

/// Set the value of Context data.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_data_replace(store: &mut wasm_store_t, new_val: *mut c_void) {
    let _old = store.context.borrow_mut().inner.data_replace(new_val);
}

/// Deletes a WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_delete(_store: Option<Box<wasm_store_t>>) {}
