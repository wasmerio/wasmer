//! Implementation of the `wasm_ref_t` reference surface of the wasm-c-api.
//!
//! A `wasm_ref_t` is a boxed `{ Value, store handle }` mirroring
//! [`wasm_extern_t`], carrying either an `externref` or a `funcref`. A `NULL`
//! `wasm_ref_t*` is the null reference; non-null references are always the
//! `Some(_)` variant.
//!
//! NOTE: references here are non-owning. The underlying extern object lives in
//! the store arena until the store is dropped, so every `wasm_ref_t` (and its
//! host info) leaks until store death. Reference lifetime management (a proper
//! reference GC) is a planned follow-up.

use std::cell::Cell;
use std::os::raw::c_void;

use wasmer_api::{ExternRef, Value};

use super::super::store::{StoreRef, WeakStoreRef, wasm_store_t};

/// Host-info payload carried inside a foreign [`ExternRef`].
///
/// Stored as the `Any` payload of the `ExternRef` so that every `wasm_ref_t`
/// handle (and every copy) pointing at the same extern object observes the same
/// host info — no store-side map is needed.
pub(crate) struct Foreign {
    host_info: Cell<*mut c_void>,
    finalizer: Cell<Option<extern "C" fn(*mut c_void)>>,
}

// SAFETY: wasm-c-api stores are single-threaded (a documented invariant of the
// C API), and `externref`/`exnref` are not shareable across threads. The raw
// host-info pointer and finalizer are therefore never accessed concurrently.
unsafe impl Send for Foreign {}
unsafe impl Sync for Foreign {}

impl Foreign {
    fn new() -> Self {
        Self {
            host_info: Cell::new(std::ptr::null_mut()),
            finalizer: Cell::new(None),
        }
    }
}

impl Drop for Foreign {
    fn drop(&mut self) {
        if let Some(finalizer) = self.finalizer.get() {
            finalizer(self.host_info.get());
        }
    }
}

/// A boxed WebAssembly reference (`externref` or `funcref`).
///
/// The store handle is [`WeakStoreRef`], not a strong `StoreRef`: some
/// references (e.g. from `wasm_func_as_ref`) are never freed by the embedder,
/// and a strong handle would pin the whole store. Operations upgrade the handle
/// and safely no-op if the store has already been dropped.
#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct wasm_ref_t {
    pub(crate) inner: Value,
    pub(crate) store: WeakStoreRef,
}

impl wasm_ref_t {
    /// Box a reference value. Returns `None` (i.e. the null `wasm_ref_t*`) for
    /// null references and non-reference values.
    pub(crate) fn new(store: StoreRef, inner: Value) -> Option<Box<wasm_ref_t>> {
        match &inner {
            Value::ExternRef(Some(_)) | Value::FuncRef(Some(_)) => Some(Box::new(wasm_ref_t {
                inner,
                store: store.downgrade(),
            })),
            _ => None,
        }
    }
}

/// Runs `f` against the [`Foreign`] payload of `ref_`, if it carries one and
/// the owning store is still alive.
fn with_foreign<R>(ref_: &wasm_ref_t, f: impl FnOnce(&Foreign) -> R) -> Option<R> {
    match &ref_.inner {
        Value::ExternRef(Some(e)) => {
            let store = ref_.store.upgrade()?;
            let store_ref = unsafe { store.store() };
            e.downcast::<Foreign>(&store_ref).map(f)
        }
        _ => None,
    }
}

/// Whether two reference values point at the same underlying object.
pub(crate) fn refs_same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::ExternRef(Some(x)), Value::ExternRef(Some(y))) => x.ptr_eq(y),
        (Value::ExternRef(None), Value::ExternRef(None)) => true,
        (Value::FuncRef(Some(x)), Value::FuncRef(Some(y))) => x == y,
        (Value::FuncRef(None), Value::FuncRef(None)) => true,
        _ => false,
    }
}

// ------------------------------------------------------------------------
// WASM_DECLARE_REF_BASE(ref)
// ------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_delete(_ref: Option<Box<wasm_ref_t>>) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_copy(ref_: Option<&wasm_ref_t>) -> Option<Box<wasm_ref_t>> {
    Some(Box::new(ref_?.clone()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_same(
    ref1: Option<&wasm_ref_t>,
    ref2: Option<&wasm_ref_t>,
) -> bool {
    match (ref1, ref2) {
        (Some(a), Some(b)) => refs_same(&a.inner, &b.inner),
        (None, None) => true,
        _ => false,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_get_host_info(ref_: Option<&wasm_ref_t>) -> *mut c_void {
    ref_.and_then(|r| with_foreign(r, |f| f.host_info.get()))
        .unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_set_host_info(ref_: Option<&wasm_ref_t>, info: *mut c_void) {
    if let Some(r) = ref_ {
        with_foreign(r, |f| {
            f.finalizer.set(None);
            f.host_info.set(info);
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_set_host_info_with_finalizer(
    ref_: Option<&wasm_ref_t>,
    info: *mut c_void,
    finalizer: Option<extern "C" fn(*mut c_void)>,
) {
    if let Some(r) = ref_ {
        with_foreign(r, |f| {
            f.host_info.set(info);
            f.finalizer.set(finalizer);
        });
    }
}

// ------------------------------------------------------------------------
// Foreign objects: WASM_DECLARE_REF(foreign) + wasm_foreign_new
//
// `wasm_foreign_t` shares the `wasm_ref_t` representation, so the cast helpers
// are the identity and the base-family wrappers delegate to the `ref` ones.
// ------------------------------------------------------------------------

#[allow(non_camel_case_types)]
pub type wasm_foreign_t = wasm_ref_t;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_new(
    store: Option<&mut wasm_store_t>,
) -> Option<Box<wasm_foreign_t>> {
    let store = store?;
    let extern_ref = {
        let mut store_mut = unsafe { store.inner.store_mut() };
        ExternRef::new(&mut store_mut, Foreign::new())
    };
    Some(Box::new(wasm_ref_t {
        inner: Value::ExternRef(Some(extern_ref)),
        store: store.inner.downgrade(),
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_delete(_foreign: Option<Box<wasm_foreign_t>>) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_copy(
    foreign: Option<&wasm_foreign_t>,
) -> Option<Box<wasm_foreign_t>> {
    Some(Box::new(foreign?.clone()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_same(
    foreign1: Option<&wasm_foreign_t>,
    foreign2: Option<&wasm_foreign_t>,
) -> bool {
    unsafe { wasm_ref_same(foreign1, foreign2) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_get_host_info(
    foreign: Option<&wasm_foreign_t>,
) -> *mut c_void {
    unsafe { wasm_ref_get_host_info(foreign) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_set_host_info(
    foreign: Option<&wasm_foreign_t>,
    info: *mut c_void,
) {
    unsafe { wasm_ref_set_host_info(foreign, info) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_set_host_info_with_finalizer(
    foreign: Option<&wasm_foreign_t>,
    info: *mut c_void,
    finalizer: Option<extern "C" fn(*mut c_void)>,
) {
    unsafe { wasm_ref_set_host_info_with_finalizer(foreign, info, finalizer) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_as_ref(
    foreign: Option<&mut wasm_foreign_t>,
) -> Option<&mut wasm_ref_t> {
    foreign
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_foreign_as_ref_const(
    foreign: Option<&wasm_foreign_t>,
) -> Option<&wasm_ref_t> {
    foreign
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_as_foreign(
    ref_: Option<&mut wasm_ref_t>,
) -> Option<&mut wasm_foreign_t> {
    ref_
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_ref_as_foreign_const(
    ref_: Option<&wasm_ref_t>,
) -> Option<&wasm_foreign_t> {
    ref_
}

// Externref creation is only implemented on the `sys` backend.
#[cfg(all(test, feature = "sys"))]
mod tests {
    use super::*;
    use crate::wasm_c_api::engine::wasm_engine_new;
    use crate::wasm_c_api::store::wasm_store_new;

    #[test]
    fn foreign_ref_host_info_copy_and_same() {
        unsafe {
            let engine = wasm_engine_new();
            let mut store = wasm_store_new(Some(&engine)).expect("store is created");

            let foreign = wasm_foreign_new(Some(&mut store)).expect("foreign ref");

            // Host info defaults to null and round-trips through set/get.
            assert!(wasm_ref_get_host_info(Some(&foreign)).is_null());
            wasm_ref_set_host_info(Some(&foreign), 42usize as *mut c_void);
            assert_eq!(
                wasm_ref_get_host_info(Some(&foreign)),
                42usize as *mut c_void
            );

            // A copy is a distinct box referring to the same extern object, so
            // it is `same` and observes the same (shared) host info.
            let copy = wasm_ref_copy(Some(&foreign)).expect("copy");
            assert!(wasm_ref_same(Some(&foreign), Some(&copy)));
            assert_eq!(wasm_ref_get_host_info(Some(&copy)), 42usize as *mut c_void);

            // Independently minted refs are not the same; null identities hold.
            let other = wasm_foreign_new(Some(&mut store)).expect("other ref");
            assert!(!wasm_ref_same(Some(&foreign), Some(&other)));
            assert!(wasm_ref_same(None, None));
            assert!(!wasm_ref_same(Some(&foreign), None));
        }
    }
}
