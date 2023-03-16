use rusty_jsc::{JSContext, JSObject};
use std::sync::Arc;

#[derive(Debug)]
struct InnerEngine {
    context: JSContext,
    wasm_module_type: JSObject,
    wasm_instance_type: JSObject,
    wasm_global_type: JSObject,
    wasm_table_type: JSObject,
    wasm_memory_type: JSObject,
}

impl Default for InnerEngine {
    fn default() -> Self {
        let context = JSContext::default();
        let mut global = context.get_global_object();
        let mut global_wasm = global
            .get_property(&context, "WebAssembly".to_string())
            .to_object(&context);

        let mut wasm_module_type = global_wasm
            .get_property(&context, "Module".to_string())
            .to_object(&context);

        let mut wasm_instance_type = global_wasm
            .get_property(&context, "Instance".to_string())
            .to_object(&context);

        let mut wasm_global_type = global_wasm
            .get_property(&context, "Global".to_string())
            .to_object(&context);

        let mut wasm_table_type = global_wasm
            .get_property(&context, "Table".to_string())
            .to_object(&context);

        let mut wasm_memory_type = global_wasm
            .get_property(&context, "Memory".to_string())
            .to_object(&context);

        Self {
            context,
            wasm_module_type,
            wasm_instance_type,
            wasm_global_type,
            wasm_table_type,
            wasm_memory_type,
        }
    }
}

/// A WebAssembly `Universal` Engine.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    inner: Arc<InnerEngine>,
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
    pub(crate) fn wasm_module_type(&self) -> &JSObject {
        &self.inner.wasm_module_type
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

// impl Default for Engine {
//     fn default() -> Self {
//         Engine
//     }
// }

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}
