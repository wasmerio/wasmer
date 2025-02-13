use std::sync::Arc;

use rusty_jsc::{JSContext, JSObject};

use crate::{AsEngineRef, AsStoreRef};

#[derive(Debug)]
pub(crate) struct JSCEngine {
    context: JSContext,
    global_wasm: JSObject,
    wasm_validate_type: JSObject,
    wasm_module_type: JSObject,
    wasm_instance_type: JSObject,
    wasm_global_type: JSObject,
    wasm_table_type: JSObject,
    wasm_memory_type: JSObject,
}

impl Default for JSCEngine {
    fn default() -> Self {
        let context = JSContext::default();
        let mut global = context.get_global_object();
        let mut global_wasm = global
            .get_property(&context, "WebAssembly".to_string())
            .to_object(&context)
            .expect("WebAssembly is not available in JavascriptCore");

        let mut wasm_validate_type = global_wasm
            .get_property(&context, "validate".to_string())
            .to_object(&context)
            .unwrap();

        let mut wasm_module_type = global_wasm
            .get_property(&context, "Module".to_string())
            .to_object(&context)
            .unwrap();

        let mut wasm_instance_type = global_wasm
            .get_property(&context, "Instance".to_string())
            .to_object(&context)
            .unwrap();

        let mut wasm_global_type = global_wasm
            .get_property(&context, "Global".to_string())
            .to_object(&context)
            .unwrap();

        let mut wasm_table_type = global_wasm
            .get_property(&context, "Table".to_string())
            .to_object(&context)
            .unwrap();

        let mut wasm_memory_type = global_wasm
            .get_property(&context, "Memory".to_string())
            .to_object(&context)
            .unwrap();

        Self {
            context,
            global_wasm,
            wasm_validate_type,
            wasm_module_type,
            wasm_instance_type,
            wasm_global_type,
            wasm_table_type,
            wasm_memory_type,
        }
    }
}

/// The engine for the JavascriptCore runtime.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    inner: Arc<JSCEngine>,
}

unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

impl From<&crate::engine::Engine> for Engine {
    fn from(engine: &crate::engine::Engine) -> Self {
        engine.as_jsc().clone()
    }
}

pub(crate) trait IntoJSC {
    fn jsc(&self) -> &JSCEngine;
}

impl IntoJSC for crate::Engine {
    #[inline]
    fn jsc(&self) -> &JSCEngine {
        &self.as_jsc().inner
    }
}

impl IntoJSC for crate::engine::EngineRef<'_> {
    #[inline]
    fn jsc(&self) -> &JSCEngine {
        &self.engine().as_jsc().inner
    }
}

impl IntoJSC for crate::store::StoreRef<'_> {
    #[inline]
    fn jsc(&self) -> &JSCEngine {
        &self.engine().jsc()
    }
}

impl IntoJSC for crate::store::StoreMut<'_> {
    #[inline]
    fn jsc(&self) -> &JSCEngine {
        &self.engine().jsc()
    }
}

impl IntoJSC for crate::Store {
    #[inline]
    fn jsc(&self) -> &JSCEngine {
        &self.engine().jsc()
    }
}

impl JSCEngine {
    #[inline]
    pub(crate) fn context(&self) -> &JSContext {
        &self.context
    }

    #[inline]
    pub(crate) fn global_wasm(&self) -> &JSObject {
        &self.global_wasm
    }

    #[inline]
    pub(crate) fn wasm_module_type(&self) -> &JSObject {
        &self.wasm_module_type
    }

    #[inline]
    pub(crate) fn wasm_validate_type(&self) -> &JSObject {
        &self.wasm_validate_type
    }

    #[inline]
    pub(crate) fn wasm_instance_type(&self) -> &JSObject {
        &self.wasm_instance_type
    }

    #[inline]
    pub(crate) fn wasm_global_type(&self) -> &JSObject {
        &self.wasm_global_type
    }

    #[inline]
    pub(crate) fn wasm_table_type(&self) -> &JSObject {
        &self.wasm_table_type
    }

    #[inline]
    pub(crate) fn wasm_memory_type(&self) -> &JSObject {
        &self.wasm_memory_type
    }
}

impl Engine {
    pub(crate) fn deterministic_id(&self) -> &str {
        // All js engines have the same id
        "javascriptcore"
    }

    #[inline]
    pub(crate) fn context(&self) -> &JSContext {
        &self.inner.context
    }

    #[inline]
    pub(crate) fn global_wasm(&self) -> &JSObject {
        &self.inner.global_wasm
    }

    #[inline]
    pub(crate) fn wasm_module_type(&self) -> &JSObject {
        &self.inner.wasm_module_type
    }

    #[inline]
    pub(crate) fn wasm_validate_type(&self) -> &JSObject {
        &self.inner.wasm_validate_type
    }

    #[inline]
    pub(crate) fn wasm_instance_type(&self) -> &JSObject {
        &self.inner.wasm_instance_type
    }

    #[inline]
    pub(crate) fn wasm_global_type(&self) -> &JSObject {
        &self.inner.wasm_global_type
    }

    #[inline]
    pub(crate) fn wasm_table_type(&self) -> &JSObject {
        &self.inner.wasm_table_type
    }

    #[inline]
    pub(crate) fn wasm_memory_type(&self) -> &JSObject {
        &self.inner.wasm_memory_type
    }
}

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}

impl crate::Engine {
    /// Consume [`self`] into a [`crate::backend::jsc::engine::Engine`].
    pub fn into_jsc(self) -> crate::backend::jsc::engine::Engine {
        match self.be {
            crate::BackendEngine::Jsc(s) => s,
            _ => panic!("Not a `jsc` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::jsc::engine::Engine`].
    pub fn as_jsc(&self) -> &crate::backend::jsc::engine::Engine {
        match self.be {
            crate::BackendEngine::Jsc(ref s) => s,
            _ => panic!("Not a `jsc` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::jsc::engine::Engine`].
    pub fn as_jsc_mut(&mut self) -> &mut crate::backend::jsc::engine::Engine {
        match self.be {
            crate::BackendEngine::Jsc(ref mut s) => s,
            _ => panic!("Not a `jsc` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `jsc` runtime.
    pub fn is_jsc(&self) -> bool {
        matches!(self.be, crate::BackendEngine::Jsc(_))
    }
}

impl Into<crate::Engine> for Engine {
    fn into(self) -> crate::Engine {
        crate::Engine {
            be: crate::BackendEngine::Jsc(self),
            id: crate::Engine::atomic_next_engine_id(),
        }
    }
}
