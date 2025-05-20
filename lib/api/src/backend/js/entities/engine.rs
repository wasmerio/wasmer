use wasmer_types::{target::Target, Features};

/// The engine for the JavaScript runtime.
#[derive(Clone, Debug)]
pub struct Engine;

impl Engine {
    pub(crate) fn deterministic_id(&self) -> String {
        // All js engines have the same id
        String::from("js-generic")
    }

    /// Returns the WebAssembly features supported by the JS engine.
    pub fn supported_features() -> Features {
        // JS-specific features
        let mut features = Features::default();
        features.bulk_memory(true);
        features.reference_types(true);
        features.simd(true);
        features.threads(true);
        features.multi_value(true);
        features.exceptions(false);
        features
    }

    /// Returns the default features for the JS engine.
    pub fn default_features() -> Features {
        Self::supported_features()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine
    }
}

/// Returns the default engine for the JS engine
pub(crate) fn default_engine() -> Engine {
    Engine::default()
}

impl crate::Engine {
    /// Consume [`self`] into a [`crate::backend::js::engine::Engine`].
    pub fn into_js(self) -> crate::backend::js::engine::Engine {
        match self.be {
            crate::BackendEngine::Js(s) => s,
            _ => panic!("Not a `js` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::backend::js::engine::Engine`].
    pub fn as_js(&self) -> &crate::backend::js::engine::Engine {
        match self.be {
            crate::BackendEngine::Js(ref s) => s,
            _ => panic!("Not a `js` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::backend::js::engine::Engine`].
    pub fn as_js_mut(&mut self) -> &mut crate::backend::js::engine::Engine {
        match self.be {
            crate::BackendEngine::Js(ref mut s) => s,
            _ => panic!("Not a `js` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `js` runtime.
    pub fn is_js(&self) -> bool {
        matches!(self.be, crate::BackendEngine::Js(_))
    }
}

impl Into<crate::Engine> for Engine {
    fn into(self) -> crate::Engine {
        crate::Engine {
            be: crate::BackendEngine::Js(self),
            id: crate::Engine::atomic_next_engine_id(),
        }
    }
}
