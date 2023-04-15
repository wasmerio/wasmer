#![cfg_attr(feature = "sys", allow(unused))]
use std::cell::{Ref, RefCell, RefMut};
use std::ops::{Deref, DerefMut};
use std::{
    collections::HashMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use wasmer::Memory;

use crate::WasiInstanceHandles;

static LOCAL_INSTANCE_SEED: AtomicU64 = AtomicU64::new(1);
thread_local! {
    static THREAD_LOCAL_INSTANCE_HANDLES: RefCell<HashMap<u64, Rc<RefCell<WasiInstanceHandles>>>>
        = RefCell::new(HashMap::new());
}

/// This non-sendable guard provides memory safe access
/// to the WasiInstance object but only when it is
/// constructed with certain constraints
pub(crate) struct WasiInstanceGuard<'a> {
    // the order is very important as the first value is
    // dropped before the reference count is dropped
    borrow: Ref<'static, WasiInstanceHandles>,
    _pointer: &'a WasiInstanceHandlesPointer,
    _inner: Rc<RefCell<WasiInstanceHandles>>,
}
impl<'a> Deref for WasiInstanceGuard<'a> {
    type Target = WasiInstanceHandles;
    fn deref(&self) -> &Self::Target {
        self.borrow.deref()
    }
}

/// This non-sendable guard provides memory safe access
/// to the WasiInstance object but only when it is
/// constructed with certain constraints. This one provides
/// mutable access
pub(crate) struct WasiInstanceGuardMut<'a> {
    // the order is very important as the first value is
    // dropped before the reference count is dropped
    borrow: RefMut<'static, WasiInstanceHandles>,
    _pointer: &'a WasiInstanceHandlesPointer,
    _inner: Rc<RefCell<WasiInstanceHandles>>,
}
impl<'a> Deref for WasiInstanceGuardMut<'a> {
    type Target = WasiInstanceHandles;

    fn deref(&self) -> &Self::Target {
        self.borrow.deref()
    }
}
impl<'a> DerefMut for WasiInstanceGuardMut<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.borrow.deref_mut()
    }
}

/// This handle protects the WasiInstance and makes it
/// accessible only when you are in the current thread
/// otherwise it will return None. This means it becomes
/// possible to make WasiEnv send without unsafe code
/// however it means that access to the must be checked
#[derive(Debug, Default, Clone)]
pub(crate) struct WasiInstanceHandlesPointer {
    /// Inner functions and references that are loaded before the environment starts
    id: Option<u64>,
}
impl Drop for WasiInstanceHandlesPointer {
    fn drop(&mut self) {
        self.clear();
    }
}
impl WasiInstanceHandlesPointer {
    pub fn get(&self) -> Option<WasiInstanceGuard<'_>> {
        self.id
            .iter()
            .filter_map(|id| {
                THREAD_LOCAL_INSTANCE_HANDLES.with(|map| {
                    let map = map.borrow();
                    if let Some(inner) = map.get(id) {
                        let borrow: Ref<WasiInstanceHandles> = inner.borrow();
                        let borrow: Ref<'static, WasiInstanceHandles> =
                            unsafe { std::mem::transmute(borrow) };
                        Some(WasiInstanceGuard {
                            borrow,
                            _pointer: self,
                            _inner: inner.clone(),
                        })
                    } else {
                        None
                    }
                })
            })
            .next()
    }
    pub fn get_mut(&self) -> Option<WasiInstanceGuardMut<'_>> {
        self.id
            .into_iter()
            .filter_map(|id| {
                THREAD_LOCAL_INSTANCE_HANDLES.with(|map| {
                    let map = map.borrow_mut();
                    if let Some(inner) = map.get(&id) {
                        let borrow: RefMut<WasiInstanceHandles> = inner.borrow_mut();
                        let borrow: RefMut<'static, WasiInstanceHandles> =
                            unsafe { std::mem::transmute(borrow) };
                        Some(WasiInstanceGuardMut {
                            borrow,
                            _pointer: self,
                            _inner: inner.clone(),
                        })
                    } else {
                        None
                    }
                })
            })
            .next()
    }
    pub fn set(&mut self, val: WasiInstanceHandles) {
        self.clear();

        let id = LOCAL_INSTANCE_SEED.fetch_add(1, Ordering::SeqCst);
        THREAD_LOCAL_INSTANCE_HANDLES.with(|map| {
            let mut map = map.borrow_mut();
            map.insert(id, Rc::new(RefCell::new(val)));
        });
        if let Some(old_id) = self.id.replace(id) {
            Self::destroy(old_id)
        }
    }
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        if let Some(id) = self.id.take() {
            Self::destroy(id)
        }
    }
    fn destroy(id: u64) {
        THREAD_LOCAL_INSTANCE_HANDLES.with(|map| {
            let mut map = map.borrow_mut();
            map.remove(&id);
        })
    }
}

/// This provides access to the memory inside the instance
pub(crate) struct WasiInstanceGuardMemory<'a> {
    // the order is very important as the first value is
    // dropped before the reference count is dropped
    borrow: &'a Memory,
    _guard: WasiInstanceGuard<'a>,
}
impl<'a> Deref for WasiInstanceGuardMemory<'a> {
    type Target = Memory;
    fn deref(&self) -> &Self::Target {
        self.borrow
    }
}
impl<'a> WasiInstanceGuard<'a> {
    pub fn memory(self) -> WasiInstanceGuardMemory<'a> {
        let borrow: &Memory = &self.memory;
        let borrow: &'a Memory = unsafe { std::mem::transmute(borrow) };
        WasiInstanceGuardMemory {
            borrow,
            _guard: self,
        }
    }
}
