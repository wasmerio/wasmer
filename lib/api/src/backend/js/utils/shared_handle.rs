//! Shared Rust ownership with worker-local JavaScript references.
use js_sys::{Array, SharedArrayBuffer, WebAssembly};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU32, Ordering},
    },
};
use wasm_bindgen::{JsCast, JsValue};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static OWNERS: LazyLock<Mutex<HashMap<u32, Weak<Owner>>>> = LazyLock::new(Mutex::default);
// A routing namespace for task envelopes, not an authentication credential.
static NAMESPACE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "wasmer-shared-objects-v1-{}-{}-{}",
        js_sys::Date::now(),
        js_sys::Math::random(),
        js_sys::Math::random()
    )
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectKind {
    Module,
    Memory,
}
impl ObjectKind {
    fn of(value: &JsValue) -> Option<Self> {
        if value.is_instance_of::<WebAssembly::Module>() {
            return Some(Self::Module);
        }
        value
            .dyn_ref::<WebAssembly::Memory>()
            .filter(|memory| memory.buffer().is_instance_of::<SharedArrayBuffer>())
            .map(|_| Self::Memory)
    }
}

#[derive(Debug)]
struct Owner {
    kind: ObjectKind,
}
struct LocalObject {
    // Rust ownership is invisible to JS GC, so a live handle needs a strong JS root.
    value: JsValue,
    owner: Weak<Owner>,
}
thread_local! {
    static OBJECTS: RefCell<HashMap<u32, LocalObject>> = RefCell::default();
    static FALLBACKS: RefCell<u32> = const { RefCell::new(0) };
}

#[derive(Clone, Debug)]
pub(crate) struct SharedJsHandle {
    id: u32,
    owner: Arc<Owner>,
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
        let kind = ObjectKind::of(&value).expect("only modules and shared memories can be shared");
        OBJECTS.with_borrow_mut(|objects| {
            for (&id, object) in objects.iter() {
                if object.value == value
                    && let Some(owner) = object.owner.upgrade()
                {
                    return Self { id, owner };
                }
            }
            let id = NEXT_ID
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("shared JavaScript handle IDs exhausted");
            let owner = Arc::new(Owner { kind });
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
        let value = value.into();
        assert_eq!(ObjectKind::of(&value), Some(self.owner.kind));
        OBJECTS.with_borrow_mut(|objects| {
            objects.insert(
                self.id,
                LocalObject {
                    value,
                    owner: Arc::downgrade(&self.owner),
                },
            );
        });
        FALLBACKS.with_borrow_mut(|count| *count += 1);
    }
}

/// Release dead references in this worker and expired entries in the owner index.
///
/// Collection is opportunistic: registration and snapshot boundaries call this;
/// hosts may also call it at task boundaries. There are no background notifications
/// or timers. Idle workers can retain expired objects until collection or teardown.
pub fn collect_shared_objects() {
    OBJECTS.with_borrow_mut(|objects| objects.retain(|_, object| object.owner.strong_count() > 0));
    OWNERS
        .lock()
        .unwrap()
        .retain(|_, owner| owner.strong_count() > 0);
}

/// Export all live local objects for structured cloning within one trusted runtime.
pub fn export_shared_objects() -> Array {
    collect_shared_objects();
    OBJECTS.with_borrow(|objects| {
        let snapshot = Array::new();
        for (&id, object) in objects {
            snapshot.push(&Array::of2(&JsValue::from(id), &object.value));
        }
        snapshot
    })
}

/// Install a snapshot, validating the whole batch before modifying the registry.
/// Expired IDs are ignored; malformed IDs, duplicates and wrong types are errors.
///
/// # Safety
/// Values must be the objects associated with these IDs by this runtime,
/// delivered through structured cloning. Validation cannot authenticate JS objects.
pub unsafe fn import_shared_objects(snapshot: &Array) -> Result<(), JsValue> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for entry in snapshot.iter() {
        if !Array::is_array(&entry) {
            return Err(JsValue::from_str("invalid shared-object entry"));
        }
        let entry: Array = entry.unchecked_into();
        if entry.length() != 2 {
            return Err(JsValue::from_str("invalid shared-object entry length"));
        }
        let id = entry
            .get(0)
            .as_f64()
            .filter(|id| {
                id.is_finite() && *id >= 1.0 && *id <= f64::from(u32::MAX) && id.fract() == 0.0
            })
            .ok_or_else(|| JsValue::from_str("invalid shared-object ID"))? as u32;
        if !seen.insert(id) {
            return Err(JsValue::from_str("duplicate shared-object ID"));
        }
        let value = entry.get(1);
        let kind = ObjectKind::of(&value)
            .ok_or_else(|| JsValue::from_str("invalid shared-object type"))?;
        let owner = OWNERS.lock().unwrap().get(&id).and_then(Weak::upgrade);
        if let Some(owner) = owner {
            if owner.kind != kind {
                return Err(JsValue::from_str("shared-object type mismatch"));
            }
            entries.push((id, value, owner));
        }
    }
    collect_shared_objects();
    OBJECTS.with_borrow_mut(|objects| {
        for (id, value, owner) in &entries {
            objects.entry(*id).or_insert_with(|| LocalObject {
                value: value.clone(),
                owner: Arc::downgrade(owner),
            });
        }
    });
    Ok(())
}

/// Wrap a task payload with all live local modules and shared memories.
///
/// Post this envelope, then import it with receive_shared_object_message before
/// accessing the task. No per-connection state is retained: retries and new workers
/// use the same full-snapshot protocol. Plain lifecycle messages need no envelope.
///
/// This is a trusted-pool protocol, not per-task capability isolation. It guarantees
/// availability of the attached objects at dispatch, not modules published later
/// through shared Rust state while a worker is executing synchronously.
///
/// ```ignore
/// worker.post_message(&prepare_shared_object_message(payload))?;
/// let payload = unsafe { receive_shared_object_message(event.data())? };
/// ```
pub fn prepare_shared_object_message(payload: JsValue) -> JsValue {
    Array::of3(
        &JsValue::from_str(&NAMESPACE),
        &payload,
        &export_shared_objects(),
    )
    .into()
}

/// Import a transport envelope and return its application payload.
/// Plain non-array lifecycle messages pass through unchanged.
///
/// # Safety
/// The envelope must be produced by this runtime's prepare_shared_object_message on the
/// same trusted worker connection; see import_shared_objects.
pub unsafe fn receive_shared_object_message(message: JsValue) -> Result<JsValue, JsValue> {
    if !Array::is_array(&message) {
        return Ok(message);
    }
    let envelope: Array = message.unchecked_into();
    if envelope.length() != 3 || envelope.get(0).as_string().as_deref() != Some(NAMESPACE.as_str())
    {
        return Err(JsValue::from_str(
            "invalid shared-object envelope or runtime namespace",
        ));
    }
    let objects = envelope.get(2);
    if !Array::is_array(&objects) {
        return Err(JsValue::from_str("invalid shared-object snapshot"));
    }
    unsafe { import_shared_objects(&objects.unchecked_into())? };
    Ok(envelope.get(1))
}

/// Diagnostic counters without performing collection.
#[doc(hidden)]
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

    fn module_handle() -> SharedJsHandle {
        SharedJsHandle::new(
            js_sys::WebAssembly::Module::new(
                &js_sys::Uint8Array::from(b"\0asm\x01\0\0\0".as_slice()).into(),
            )
            .unwrap(),
        )
    }

    #[wasm_bindgen_test]
    fn snapshots_resend_live_objects_and_omit_expired_ones() {
        let handle = module_handle();
        for _ in 0..3 {
            let message = prepare_shared_object_message(JsValue::UNDEFINED);
            let envelope = Array::from(&message);
            assert_eq!(Array::from(&envelope.get(2)).length(), 1);
            unsafe { receive_shared_object_message(message) }.unwrap();
        }
        drop(handle);
        let message = Array::from(&prepare_shared_object_message(JsValue::UNDEFINED));
        assert_eq!(Array::from(&message.get(2)).length(), 0);
    }

    #[wasm_bindgen_test]
    fn malformed_snapshot_is_rejected_atomically() {
        let handle = module_handle();
        let snapshot = export_shared_objects();
        OBJECTS.with_borrow_mut(|objects| objects.remove(&handle.id));
        let malformed = Array::from(snapshot.as_ref());
        malformed.push(&Array::of2(&JsValue::from(1.5), &JsValue::NULL));
        assert!(unsafe { import_shared_objects(&malformed) }.is_err());
        assert!(handle.get::<WebAssembly::Module>().is_none());
        let duplicate = Array::from(snapshot.as_ref());
        duplicate.push(&snapshot.get(0));
        assert!(unsafe { import_shared_objects(&duplicate) }.is_err());
        assert!(handle.get::<WebAssembly::Module>().is_none());
        unsafe { import_shared_objects(&snapshot) }.unwrap();
        assert!(handle.get::<WebAssembly::Module>().is_some());
    }

    #[wasm_bindgen_test]
    fn expired_snapshots_do_not_resurrect_handles() {
        let handle = module_handle();
        let id = handle.id;
        let snapshot = export_shared_objects();
        drop(handle);
        collect_shared_objects();
        unsafe { import_shared_objects(&snapshot) }.unwrap();
        assert!(!OBJECTS.with_borrow(|objects| objects.contains_key(&id)));
    }

    #[wasm_bindgen_test]
    fn snapshots_reject_invalid_ids_and_object_kinds() {
        let handle = module_handle();
        let module: JsValue = handle.get::<WebAssembly::Module>().unwrap().into();
        for id in [0.0, -1.0, 1.5, f64::NAN, f64::INFINITY, 4_294_967_296.0] {
            let snapshot = Array::of1(&Array::of2(&JsValue::from(id), &module));
            assert!(unsafe { import_shared_objects(&snapshot) }.is_err());
        }
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(&descriptor, &"initial".into(), &1.into()).unwrap();
        js_sys::Reflect::set(&descriptor, &"maximum".into(), &2.into()).unwrap();
        for shared in [false, true] {
            js_sys::Reflect::set(&descriptor, &"shared".into(), &shared.into()).unwrap();
            let memory = WebAssembly::Memory::new(&descriptor).unwrap();
            let snapshot = Array::of1(&Array::of2(&handle.id.into(), &memory));
            // Nonshared memory is not transportable; shared memory is the wrong
            // kind for this live module ID.
            assert!(unsafe { import_shared_objects(&snapshot) }.is_err());
        }
        assert_eq!(
            JsValue::from(handle.get::<WebAssembly::Module>().unwrap()),
            module
        );
    }

    #[wasm_bindgen_test]
    fn envelopes_reject_other_runtimes_and_preserve_payloads() {
        let message = prepare_shared_object_message(JsValue::from_str("payload"));
        assert_eq!(
            unsafe { receive_shared_object_message(message.clone()) }.unwrap(),
            "payload"
        );
        let foreign = Array::from(&message);
        foreign.set(0, JsValue::from_str("another-runtime"));
        assert!(unsafe { receive_shared_object_message(foreign.into()) }.is_err());
    }

    #[wasm_bindgen_test]
    async fn final_drop_retains_local_reference_until_collection() {
        let handle = module_handle();
        let id = handle.id;
        drop(handle);
        wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED))
            .await
            .unwrap();
        assert!(OBJECTS.with_borrow(|objects| objects.contains_key(&id)));
        assert!(OWNERS.lock().unwrap().get(&id).unwrap().upgrade().is_none());
        collect_shared_objects();
        assert!(!OBJECTS.with_borrow(|objects| objects.contains_key(&id)));
        assert!(!OWNERS.lock().unwrap().contains_key(&id));
    }

    #[wasm_bindgen_test]
    fn shared_handles_preserve_identity_and_rust_ownership() {
        let js = js_sys::WebAssembly::Module::new(
            &js_sys::Uint8Array::from(b"\0asm\x01\0\0\0".as_slice()).into(),
        )
        .unwrap();
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
        let value = js_sys::WebAssembly::Module::new(
            &js_sys::Uint8Array::from(b"\0asm\x01\0\0\0".as_slice()).into(),
        )
        .unwrap();
        let handle = SharedJsHandle::new(value);
        let snapshot = export_shared_objects();
        OBJECTS.with_borrow_mut(|objects| objects.remove(&handle.id));
        assert!(handle.get::<js_sys::WebAssembly::Module>().is_none());
        // This unit test models receive-side registration in one realm.
        // SDK browser integration separately exercises real postMessage.
        unsafe { import_shared_objects(&snapshot) }.unwrap();
        assert!(handle.get::<js_sys::WebAssembly::Module>().is_some());
    }

    #[wasm_bindgen_test]
    fn receive_installs_module_before_returning_payload_without_fallback() {
        let js = WebAssembly::Module::new(
            &js_sys::Uint8Array::from(b"\0asm\x01\0\0\0".as_slice()).into(),
        )
        .unwrap();
        // Modules wrapped from JS have no retained bytes to fall back to.
        let module = crate::js::module::Module::from(js.clone());
        let message = prepare_shared_object_message(JsValue::from_str("run"));
        OBJECTS.with_borrow_mut(HashMap::clear);
        let fallbacks = shared_object_stats().1;
        let payload = unsafe { receive_shared_object_message(message.clone()) }.unwrap();
        assert_eq!(payload, "run");
        assert_eq!(JsValue::from(module), JsValue::from(js));
        assert_eq!(shared_object_stats().1, fallbacks);
    }

    #[test]
    fn shared_wrappers_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SharedJsHandle>();
        assert_send_sync::<crate::Module>();
        assert_send_sync::<crate::SharedMemory>();
    }
}
