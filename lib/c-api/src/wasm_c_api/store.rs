use super::engine::wasm_engine_t;
use std::cell::UnsafeCell;
use std::rc::{Rc, Weak};
use wasmer_api::{AsStoreMut, AsStoreRef, Store, StoreMut, StoreRef as BaseStoreRef};

#[derive(Clone)]
pub struct StoreRef {
    inner: Rc<UnsafeCell<Store>>,
}

impl StoreRef {
    pub unsafe fn store(&self) -> BaseStoreRef<'_> {
        unsafe { (*self.inner.get()).as_store_ref() }
    }

    pub unsafe fn store_mut(&mut self) -> StoreMut<'_> {
        unsafe { (*self.inner.get()).as_store_mut() }
    }

    /// Create a non-owning handle to this store.
    ///
    /// Host-function callbacks must not capture a strong [`StoreRef`]: the
    /// function lives inside the store's arena, so a strong clone would form a
    /// store → function → store cycle and leak the whole store. Callbacks
    /// capture a [`WeakStoreRef`] instead and upgrade it per call.
    pub fn downgrade(&self) -> WeakStoreRef {
        WeakStoreRef {
            inner: Rc::downgrade(&self.inner),
        }
    }
}

/// A non-owning handle to a store, held by host-function callbacks.
#[derive(Clone)]
pub struct WeakStoreRef {
    inner: Weak<UnsafeCell<Store>>,
}

// SAFETY: wasm-c-api stores are single-threaded (a documented invariant of the
// C API). Host callbacks are `Send + Sync`-bound by `Function::new_with_env`,
// so this handle must be too; it is never actually shared across threads.
unsafe impl Send for WeakStoreRef {}
unsafe impl Sync for WeakStoreRef {}

impl WeakStoreRef {
    /// Upgrade to a strong [`StoreRef`], or `None` if the store has been
    /// dropped (which cannot happen while one of its callbacks is running).
    pub fn upgrade(&self) -> Option<StoreRef> {
        self.inner.upgrade().map(|inner| StoreRef { inner })
    }
}

/// Opaque type representing a WebAssembly store.
#[allow(non_camel_case_types)]
pub struct wasm_store_t {
    pub(crate) inner: StoreRef,
}

/// Creates a new WebAssembly store given a specific [engine][super::engine].
///
/// # Example
///
/// See the module's documentation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_store_new(
    engine: Option<&wasm_engine_t>,
) -> Option<Box<wasm_store_t>> {
    let engine = engine?;
    let store = Store::new(engine.inner.clone());

    Some(Box::new(wasm_store_t {
        inner: StoreRef {
            inner: Rc::new(UnsafeCell::new(store)),
        },
    }))
}

/// Deletes a WebAssembly store.
///
/// # Example
///
/// See the module's documentation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_store_delete(_store: Option<Box<wasm_store_t>>) {}
