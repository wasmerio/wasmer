use crate::Tunables;
use wasmer_compiler::Engine;

/// A temporary handle to an [`Engine`] and [`Tunables`].
/// EngineRef can be used to build a [`Module`]
/// It can be created directly with an [`Engine`] and [`Tunables`]
/// Or from anything implementing [`AsEngineRef`]
/// like from [`Store`] typicaly
pub struct EngineRef<'a> {
    /// The inner engine
    pub(crate) inner: &'a Engine,
    /// optionnal tunnables
    pub(crate) tunables: &'a dyn Tunables,
}

impl<'a> EngineRef<'a> {
    /// Get inner [`Engine`]
    pub fn engine(&self) -> &Engine {
        self.inner
    }
    /// Get the [`Tunables`]
    pub fn tunables(&self) -> &dyn Tunables {
        self.tunables
    }
    /// Create an EngineRef from an Engine and Tunables
    pub fn new(engine: &'a Engine, tunables: &'a dyn Tunables) -> Self {
        EngineRef {
            inner: engine,
            tunables,
        }
    }
}

/// Helper trait for a value that is convertible to a [`EngineRef`].
pub trait AsEngineRef {
    /// Returns a `EngineRef` pointing to the underlying context.
    fn as_engine_ref(&self) -> EngineRef<'_>;
}

impl AsEngineRef for EngineRef<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef {
            inner: self.inner,
            tunables: self.tunables,
        }
    }
}
