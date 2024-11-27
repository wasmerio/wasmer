/// The engine for the JavaScript runtime.
#[derive(Clone, Debug)]
pub struct Engine;

impl Engine {
    pub(crate) fn deterministic_id(&self) -> &str {
        // All js engines have the same id
        "js-generic"
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
    /// Consume [`self`] into a [`crate::rt::js::engine::Engine`].
    pub fn into_js(self) -> crate::rt::js::engine::Engine {
        match self.rt {
            crate::RuntimeEngine::Js(s) => s,
            _ => panic!("Not a `js` engine!"),
        }
    }

    /// Convert a reference to [`self`] into a reference [`crate::rt::js::engine::Engine`].
    pub fn as_js(&self) -> &crate::rt::js::engine::Engine {
        match self.rt {
            crate::RuntimeEngine::Js(ref s) => s,
            _ => panic!("Not a `js` engine!"),
        }
    }

    /// Convert a mutable reference to [`self`] into a mutable reference [`crate::rt::js::engine::Engine`].
    pub fn as_js_mut(&mut self) -> &mut crate::rt::js::engine::Engine {
        match self.rt {
            crate::RuntimeEngine::Js(ref mut s) => s,
            _ => panic!("Not a `js` engine!"),
        }
    }

    /// Return true if [`self`] is an engine from the `js` runtime.
    pub fn is_js(&self) -> bool {
        matches!(self.rt, RuntimeEngine::Js(_))
    }
}

impl Into<crate::Engine> for Engine {
    fn into(self) -> crate::Engine {
        crate::Engine {
            rt: crate::RuntimeEngine::Js(self),
            id: crate::Engine::atomic_next_engine_id(),
        }
    }
}
