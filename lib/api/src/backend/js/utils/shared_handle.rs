//! Shared Rust ownership with worker-local JavaScript references.
use super::shared_handle_cleanup;
use js_sys::{Array, SharedArrayBuffer, WebAssembly};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::{Rc, Weak as LocalWeak},
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU32, Ordering},
    },
};
use wasm_bindgen::{JsCast, JsValue};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static OWNERS: LazyLock<Mutex<HashMap<u32, Weak<Owner>>>> = LazyLock::new(Mutex::default);
// A routing namespace, not an authentication credential. Notifications only
// request collection; each recipient checks its own Rust ownership.
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
impl Drop for Owner {
    fn drop(&mut self) {
        shared_handle_cleanup::notify(&NAMESPACE);
    }
}
struct LocalObject {
    value: JsValue,
    owner: Weak<Owner>,
}
type KnownObjects = RefCell<HashSet<u32>>;
thread_local! {
    static OBJECTS: RefCell<HashMap<u32, LocalObject>> = RefCell::default();
    static TRANSPORTS: RefCell<Vec<LocalWeak<KnownObjects>>> = RefCell::default();
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
        let handle = OBJECTS.with_borrow_mut(|objects| {
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
        });
        shared_handle_cleanup::listen(&NAMESPACE);
        handle
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
        shared_handle_cleanup::listen(&NAMESPACE);
    }
}

/// Release dead references and delivery bookkeeping in this worker.
pub fn collect_shared_objects() {
    OBJECTS.with_borrow_mut(|objects| objects.retain(|_, object| object.owner.strong_count() > 0));
    let live: HashSet<_> = {
        let mut owners = OWNERS.lock().unwrap();
        owners.retain(|_, owner| owner.strong_count() > 0);
        owners.keys().copied().collect()
    };
    TRANSPORTS.with_borrow_mut(|transports| {
        transports.retain(|transport| {
            if let Some(known) = transport.upgrade() {
                known.borrow_mut().retain(|id| live.contains(id));
                true
            } else {
                false
            }
        })
    });
}

pub(super) fn has_local_objects() -> bool {
    OBJECTS.with_borrow(|objects| !objects.is_empty())
}

fn snapshot_excluding(known: &HashSet<u32>) -> (Array, Vec<u32>) {
    OBJECTS.with_borrow(|objects| {
        let snapshot = Array::new();
        let mut ids = Vec::new();
        for (&id, object) in objects {
            if known.contains(&id) {
                continue;
            }
            let entry = Array::new();
            entry.push(&JsValue::from(id));
            entry.push(&object.value);
            snapshot.push(&entry);
            ids.push(id);
        }
        (snapshot, ids)
    })
}

/// Export all live local objects. Prefer SharedObjectTransport for repeated sends.
/// Contains shared memories: use only inside one trusted runtime/worker pool.
pub fn export_shared_objects() -> Array {
    collect_shared_objects();
    snapshot_excluding(&HashSet::new()).0
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
    if has_local_objects() {
        shared_handle_cleanup::listen(&NAMESPACE);
    }
    Ok(())
}

/// Incremental delivery state for one ordered postMessage connection.
///
/// Keep one instance per receiving worker/connection, and discard it when that
/// worker is replaced. It is intentionally local, not Send or Sync. The first
/// send includes all live local objects; later sends omit successfully sent IDs.
/// Receivers must import every message before running its task.
///
/// This is a trusted-pool protocol, not an object-capability boundary: snapshots
/// are not restricted to objects reachable from the application payload.
///
/// ```ignore
/// let prepared = transport.prepare(payload);
/// worker.post_message(prepared.message())?;
/// prepared.sent(); // Never commit on a failed postMessage.
/// // Receiver, before accessing any shared Rust task:
/// let payload = unsafe { receive_shared_object_message(event.data())? };
/// ```
#[derive(Debug)]
pub struct SharedObjectTransport {
    known: Rc<KnownObjects>,
}
impl Default for SharedObjectTransport {
    fn default() -> Self {
        let known = Rc::new(RefCell::default());
        TRANSPORTS.with_borrow_mut(|transports| transports.push(Rc::downgrade(&known)));
        Self { known }
    }
}
impl SharedObjectTransport {
    /// Prepare an envelope without advancing delivery state. Call sent only
    /// after postMessage succeeds; dropping a failed preparation permits retry.
    pub fn prepare(&self, payload: JsValue) -> PreparedSharedObjects {
        collect_shared_objects();
        let (objects, ids) = snapshot_excluding(&self.known.borrow());
        let message = Array::new();
        message.push(&JsValue::from_str(&NAMESPACE));
        message.push(&payload);
        message.push(&objects);
        PreparedSharedObjects {
            message: message.into(),
            ids,
            known: self.known.clone(),
        }
    }
}

/// A local envelope awaiting a successful postMessage call.
#[must_use = "post the message, then call sent; drop it on send failure"]
pub struct PreparedSharedObjects {
    message: JsValue,
    ids: Vec<u32>,
    known: Rc<KnownObjects>,
}
impl PreparedSharedObjects {
    /// Envelope to structured-clone through postMessage.
    pub fn message(&self) -> &JsValue {
        &self.message
    }
    /// Number of objects actually attached to this message.
    pub fn object_count(&self) -> usize {
        self.ids.len()
    }
    /// Commit only after this envelope was successfully posted on its connection.
    pub fn sent(self) {
        self.known.borrow_mut().extend(self.ids);
    }
}

/// Import a transport envelope and return its application payload.
/// Plain non-array lifecycle messages pass through unchanged.
///
/// # Safety
/// The envelope must be produced by this runtime's SharedObjectTransport on the
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
    fn transport_retries_failed_sends_and_skips_delivered_objects() {
        let handle = module_handle();
        let transport = SharedObjectTransport::default();
        let failed = transport.prepare(JsValue::UNDEFINED);
        assert_eq!(failed.object_count(), 1);
        drop(failed);
        let retry = transport.prepare(JsValue::UNDEFINED);
        assert_eq!(retry.object_count(), 1);
        retry.sent();
        assert_eq!(transport.prepare(JsValue::UNDEFINED).object_count(), 0);
        assert_eq!(
            SharedObjectTransport::default()
                .prepare(JsValue::UNDEFINED)
                .object_count(),
            1
        );
        drop(handle);
        collect_shared_objects();
        assert!(transport.known.borrow().is_empty());
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
        for id in [
            0.0,
            -1.0,
            1.5,
            f64::NAN,
            f64::INFINITY,
            f64::from(u32::MAX) + 1.0,
        ] {
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
        let transport = SharedObjectTransport::default();
        let prepared = transport.prepare(JsValue::from_str("payload"));
        assert_eq!(
            unsafe { receive_shared_object_message(prepared.message().clone()) }.unwrap(),
            "payload"
        );
        let foreign = Array::from(prepared.message());
        foreign.set(0, JsValue::from_str("another-runtime"));
        assert!(unsafe { receive_shared_object_message(foreign.into()) }.is_err());
    }

    #[wasm_bindgen_test]
    async fn final_drop_schedules_local_collection_without_a_task_boundary() {
        let handle = module_handle();
        let id = handle.id;
        drop(handle);
        wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED))
            .await
            .unwrap();
        assert!(!OBJECTS.with_borrow(|objects| objects.contains_key(&id)));
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

    #[test]
    fn shared_wrappers_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SharedJsHandle>();
        assert_send_sync::<crate::Module>();
        assert_send_sync::<crate::SharedMemory>();
    }
}
