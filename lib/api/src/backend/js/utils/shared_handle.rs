//! Prototype: Rust ownership is shared; JavaScript references stay worker-local.
use js_sys::Array;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU32, Ordering},
    },
};
use wasm_bindgen::{JsCast, JsValue};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static OWNERS: LazyLock<Mutex<HashMap<u32, Weak<()>>>> = LazyLock::new(Mutex::default);
struct LocalObject {
    value: JsValue,
    owner: Weak<()>,
}
thread_local! {
    static OBJECTS: RefCell<HashMap<u32, LocalObject>> = RefCell::default();
    static FALLBACKS: RefCell<u32> = const { RefCell::new(0) };
}

#[derive(Clone, Debug)]
pub(crate) struct SharedJsHandle {
    id: u32,
    owner: Arc<()>,
}
impl PartialEq for SharedJsHandle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for SharedJsHandle {}
impl SharedJsHandle {
    pub fn new(value: impl Into<JsValue>) -> Self {
        collect_shared_objects();
        let value = value.into();
        OBJECTS.with_borrow_mut(|objects| {
            // Preserve identity when an existing JS object is wrapped again.
            for (&id, object) in objects.iter() {
                if object.value == value && let Some(owner) = object.owner.upgrade() {
                    return Self { id, owner };
                }
            }
            let id = NEXT_ID
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("shared JavaScript handle IDs exhausted");
            let owner = Arc::new(());
            let weak = Arc::downgrade(&owner);
            OWNERS.lock().unwrap().insert(id, weak.clone());
            objects.insert(id, LocalObject { value, owner: weak });
            Self { id, owner }
        })
    }
    pub fn get<T: JsCast>(&self) -> Option<T> {
        OBJECTS.with_borrow(|objects| {
            objects.get(&self.id).map(|object| {
                object
                    .value
                    .clone()
                    .dyn_into()
                    .unwrap_or_else(|_| panic!("shared JavaScript handle has the wrong type"))
            })
        })
    }
    pub fn install(&self, value: impl Into<JsValue>) {
        OBJECTS.with_borrow_mut(|objects| {
            objects.insert(
                self.id,
                LocalObject {
                    value: value.into(),
                    owner: Arc::downgrade(&self.owner),
                },
            );
        });
        FALLBACKS.with_borrow_mut(|count| *count += 1);
    }
}

/// Release dead handles in this worker. Never touches another worker's JS table.
/// Prototype reclamation is boundary-driven, not immediate on the final Rust drop.
pub fn collect_shared_objects() {
    OBJECTS.with_borrow_mut(|objects| objects.retain(|_, object| object.owner.strong_count() > 0));
    OWNERS
        .lock()
        .unwrap()
        .retain(|_, owner| owner.strong_count() > 0);
}

/// Attach this snapshot to a worker message before executing its Rust payload.
/// It can contain shared memories; keep it inside the same trusted worker pool.
pub fn export_shared_objects() -> Array {
    collect_shared_objects();
    OBJECTS.with_borrow(|objects| {
        let snapshot = Array::new();
        for (&id, object) in objects {
            let entry = Array::new();
            entry.push(&JsValue::from(id));
            entry.push(&object.value);
            snapshot.push(&entry);
        }
        snapshot
    })
}

/// Install objects cloned by `postMessage` into this worker's handle table.
///
/// # Safety
/// The snapshot must come from `export_shared_objects` in this same runtime,
/// through structured cloning. Arbitrary input could substitute a live handle.
pub unsafe fn import_shared_objects(snapshot: &Array) {
    collect_shared_objects();
    OBJECTS.with_borrow_mut(|objects| {
        let owners = OWNERS.lock().unwrap();
        for entry in snapshot.iter() {
            let entry = Array::from(&entry);
            let id = entry.get(0).as_f64().expect("invalid shared handle ID") as u32;
            if let Some(owner) = owners.get(&id).filter(|owner| owner.strong_count() > 0) {
                objects.entry(id).or_insert_with(|| LocalObject {
                    value: entry.get(1),
                    owner: owner.clone(),
                });
            }
        }
    });
}

/// Prototype diagnostics: [local entries, synchronous module fallbacks].
pub fn shared_object_stats() -> (usize, u32) {
    (
        OBJECTS.with_borrow(|objects| objects.len()),
        FALLBACKS.with_borrow(|count| *count),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn shared_handles_preserve_identity_and_rust_ownership() {
        let js = js_sys::WebAssembly::Module::new(&js_sys::Uint8Array::from(
            b"\0asm\x01\0\0\0".as_slice(),
        ).into()).unwrap();
        let handle = SharedJsHandle::new(js.clone());
        let clone = handle.clone();
        assert_eq!(handle, SharedJsHandle::new(js));
        let id = handle.id;
        drop(handle);
        collect_shared_objects();
        assert!(clone.get::<js_sys::WebAssembly::Module>().is_some());
        drop(clone);
        collect_shared_objects();
        assert!(!OBJECTS.with_borrow(|objects| objects.contains_key(&id)));
    }

    #[wasm_bindgen_test]
    fn snapshot_import_restores_local_references() {
        let value = js_sys::WebAssembly::Module::new(&js_sys::Uint8Array::from(
            b"\0asm\x01\0\0\0".as_slice(),
        ).into()).unwrap();
        let handle = SharedJsHandle::new(value);
        let snapshot = export_shared_objects();
        OBJECTS.with_borrow_mut(|objects| objects.remove(&handle.id));
        assert!(handle.get::<js_sys::WebAssembly::Module>().is_none());
        // This unit test models receive-side registration in one realm.
        // SDK browser integration separately exercises real postMessage.
        unsafe { import_shared_objects(&snapshot) };
        assert!(handle.get::<js_sys::WebAssembly::Module>().is_some());
    }

    #[test]
    fn shared_wrappers_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SharedJsHandle>();
        assert_send_sync::<crate::Module>();
        assert_send_sync::<crate::SharedMemory>();
    }
}
