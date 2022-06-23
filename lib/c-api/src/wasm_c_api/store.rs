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
    pub(crate) context: Option<Rc<RefCell<wasm_context_t>>>,
}

impl wasm_store_t {
    pub(crate) const CTX_ERR_STR: &'static str =
    "store used without a Context set; use wasm_store_context_set() after initializing your store.";
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

    Some(Box::new(wasm_store_t {
        inner: store,
        context: None,
    }))
}

/// Sets the context for this WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_context_set(
    store: Option<&mut wasm_store_t>,
    context: Option<Box<wasm_context_t>>,
) {
    let _result = (move |store: Option<&mut wasm_store_t>,
                         context: Option<Box<wasm_context_t>>|
          -> Option<()> {
        let mut store = store?;
        let context = context?;
        store.context = Some(Rc::new(RefCell::new(*context)));
        Some(())
    })(store, context);
}

/// Get the value of Context data.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_data_get(store: &wasm_store_t) -> *mut c_void {
    *store
        .context
        .as_ref()
        .expect(wasm_store_t::CTX_ERR_STR)
        .borrow()
        .inner
        .data()
}

/// Set the value of Context data.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_data_set(store: &mut wasm_store_t, new_val: *mut c_void) {
    *store
        .context
        .as_mut()
        .expect(wasm_store_t::CTX_ERR_STR)
        .borrow_mut()
        .inner
        .data_mut() = new_val;
}

/// Deletes a WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[no_mangle]
pub unsafe extern "C" fn wasm_store_delete(_store: Option<Box<wasm_store_t>>) {}
