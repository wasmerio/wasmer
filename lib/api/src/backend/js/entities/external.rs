use std::any::Any;

use js_sys::{Object, Symbol};
use wasm_bindgen::JsValue;

use crate::js::entities::store::StoreHandle;
use crate::js::utils::js_handle::JsHandle;
use crate::js::vm::VMExternRef;
use crate::store::{AsStoreMut, AsStoreRef};

#[repr(transparent)]
/// A WebAssembly `extern ref` in `js`.
pub(crate) struct ExternRefData(pub(crate) Box<dyn Any + Send + Sync>);

impl std::fmt::Debug for ExternRefData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternRefData").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct ExternRef {
    value: JsHandle<JsValue>,
    host_data: Option<StoreHandle<ExternRefData>>,
}

unsafe impl Send for ExternRef {}
unsafe impl Sync for ExternRef {}

impl ExternRef {
    pub fn new<T>(store: &mut impl AsStoreMut, value: T) -> Self
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        let handle = StoreHandle::new(
            store.objects_mut().as_js_mut(),
            ExternRefData(Box::new(value)),
        );
        let object = Object::new();
        js_sys::Reflect::set(
            &object,
            host_ref_index_key().as_ref(),
            &JsValue::from_f64(handle.internal_handle().index() as f64),
        )
        .expect("setting the externref store index should succeed");
        js_sys::Reflect::set(
            &object,
            host_ref_store_key().as_ref(),
            &JsValue::from_f64(handle.store_id().as_raw().get() as f64),
        )
        .expect("setting the externref store ID should succeed");
        Self {
            value: JsHandle::new(object.into()),
            host_data: Some(handle),
        }
    }

    pub(crate) fn from_js_value(store: &mut impl AsStoreMut, value: JsValue) -> Self {
        let host_data = host_handle_from_js_value(store, &value);
        Self {
            value: JsHandle::new(value),
            host_data,
        }
    }

    pub(crate) fn as_js_value(&self) -> JsValue {
        (*self.value).clone()
    }

    pub fn downcast<'a, T>(&self, store: &'a impl AsStoreRef) -> Option<&'a T>
    where
        T: Any + Send + Sync + 'static + Sized,
    {
        self.host_data
            .as_ref()?
            .get(store.as_store_ref().objects().as_js())
            .0
            .downcast_ref()
    }

    pub(crate) fn vm_externref(&self) -> VMExternRef {
        VMExternRef::new(self.as_js_value())
    }

    pub(crate) unsafe fn from_vm_externref(
        store: &mut impl AsStoreMut,
        vm_externref: VMExternRef,
    ) -> Self {
        Self::from_js_value(store, vm_externref.into_js_value())
    }

    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.host_data
            .as_ref()
            .is_none_or(|handle| handle.store_id() == store.as_store_ref().objects().id())
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        js_sys::Object::is(&self.value, &other.value)
    }
}

fn host_ref_index_key() -> Symbol {
    Symbol::for_("wasmer.externref-index")
}

fn host_ref_store_key() -> Symbol {
    Symbol::for_("wasmer.externref-store")
}

fn host_handle_from_js_value(
    store: &mut impl AsStoreMut,
    value: &JsValue,
) -> Option<StoreHandle<ExternRefData>> {
    let index = js_sys::Reflect::get(value, host_ref_index_key().as_ref())
        .ok()?
        .as_f64()? as usize;
    let store_id = js_sys::Reflect::get(value, host_ref_store_key().as_ref())
        .ok()?
        .as_f64()? as u64;
    let objects = store.objects_mut();
    if objects.id().as_raw().get() as u64 != store_id {
        return None;
    }
    let internal = crate::js::entities::store::InternalStoreHandle::from_index(index)?;
    Some(unsafe { StoreHandle::from_internal(objects.id(), internal) })
}
