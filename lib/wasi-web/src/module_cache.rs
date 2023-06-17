use std::{cell::RefCell, collections::HashMap};

use bytes::Bytes;
use wasm_bindgen::{JsCast, JsValue};
use wasmer::{Engine, Module};
use wasmer_wasix::runtime::module_cache::{CacheError, ModuleCache, ModuleHash};

use crate::pool::{schedule_task, WebRunCommand};

std::thread_local! {
    static CACHED_MODULES: RefCell<HashMap<(ModuleHash, String), Module>>
        = RefCell::new(HashMap::new());
}

/// A cache that will cache modules in a thread-local variable.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct WebWorkerModuleCache {}

impl WebWorkerModuleCache {
    fn lookup(&self, key: ModuleHash, deterministic_id: &str) -> Option<Module> {
        let key = (key, deterministic_id.to_string());
        CACHED_MODULES.with(|m| m.borrow().get(&key).cloned())
    }

    fn insert(&self, key: ModuleHash, module: &Module, deterministic_id: &str) -> bool {
        let key = (key, deterministic_id.to_string());
        let old = CACHED_MODULES.with(|m| m.borrow_mut().insert(key, module.clone()));
        let already_exists = old.is_some();
        already_exists
    }

    fn cache_in_main(&self, key: ModuleHash, module: &Module, deterministic_id: &str) {
        let deterministic_id = deterministic_id.to_string();
        let task = Box::new(WebRunCommand::ExecModule {
            run: Box::new(move |module| {
                let key = (key, deterministic_id);
                CACHED_MODULES.with(|m| m.borrow_mut().insert(key, module.clone()));
            }),
            module_bytes: module.serialize().unwrap(),
        });
        let task = Box::into_raw(task);

        let module = JsValue::from(module.clone())
            .dyn_into::<js_sys::WebAssembly::Module>()
            .unwrap();
        schedule_task(JsValue::from(task as u32), module, JsValue::NULL);
    }

    pub fn export() -> JsValue {
        CACHED_MODULES.with(|m| {
            // Annotation is here to prevent spurious IDE warnings.
            #[allow(unused_unsafe)]
            unsafe {
                let entries = js_sys::Array::new_with_length(m.borrow().len() as u32);

                let mut i = 0;
                for ((key, deterministic_id), module) in m.borrow().iter() {
                    let entry = js_sys::Object::new();

                    js_sys::Reflect::set(
                        &entry,
                        &"key".into(),
                        &JsValue::from(base64::encode(key.as_bytes())),
                    )
                    .unwrap();

                    js_sys::Reflect::set(
                        &entry,
                        &"deterministic_id".into(),
                        &JsValue::from(deterministic_id.clone()),
                    )
                    .unwrap();

                    js_sys::Reflect::set(&entry, &"module".into(), &JsValue::from(module.clone()))
                        .unwrap();

                    let module_bytes = Box::new(module.serialize().unwrap());
                    let module_bytes = Box::into_raw(module_bytes);
                    js_sys::Reflect::set(
                        &entry,
                        &"module_bytes".into(),
                        &JsValue::from(module_bytes as u32),
                    )
                    .unwrap();

                    entries.set(i, JsValue::from(entry));
                    i += 1;
                }

                JsValue::from(entries)
            }
        })
    }

    pub fn import(cache: JsValue) {
        CACHED_MODULES.with(|m| {
            // Annotation is here to prevent spurious IDE warnings.
            #[allow(unused_unsafe)]
            unsafe {
                let entries = JsValue::from(cache).dyn_into::<js_sys::Array>().unwrap();

                for i in 0..entries.length() {
                    let entry = entries.get(i);

                    let key = js_sys::Reflect::get(&entry, &"key".into()).unwrap();
                    let key = JsValue::as_string(&key).unwrap();
                    let key = base64::decode(key).unwrap();
                    let key: [u8; 32] = key.try_into().unwrap();
                    let key = ModuleHash::from_bytes(key);

                    let deterministic_id =
                        js_sys::Reflect::get(&entry, &"deterministic_id".into()).unwrap();
                    let deterministic_id = JsValue::as_string(&deterministic_id).unwrap();

                    let module_bytes =
                        js_sys::Reflect::get(&entry, &"module_bytes".into()).unwrap();
                    let module_bytes: u32 = module_bytes.as_f64().unwrap() as u32;
                    let module_bytes = module_bytes as *mut Bytes;
                    let module_bytes = unsafe { Box::from_raw(module_bytes) };

                    let module = js_sys::Reflect::get(&entry, &"module".into()).unwrap();
                    let module = module.dyn_into::<js_sys::WebAssembly::Module>().unwrap();
                    let module: Module = (module, *module_bytes).into();

                    let key = (key, deterministic_id);
                    m.borrow_mut().insert(key, module.clone());
                }
            }
        });
    }
}

#[async_trait::async_trait]
impl ModuleCache for WebWorkerModuleCache {
    async fn load(&self, key: ModuleHash, engine: &Engine) -> Result<Module, CacheError> {
        self.lookup(key, engine.deterministic_id())
            .ok_or(CacheError::NotFound)
    }

    async fn save(
        &self,
        key: ModuleHash,
        engine: &Engine,
        module: &Module,
    ) -> Result<(), CacheError> {
        let already_exists = self.insert(key, module, engine.deterministic_id());

        // We also send the module to the main thread via a postMessage
        // which they relays it to all the web works
        if already_exists == false {
            self.cache_in_main(key, module, engine.deterministic_id());
        }

        Ok(())
    }
}
