use core::ops::Deref;

#[cfg(feature = "sys")]
use crate::sys::engine as engine_imp;
#[cfg(feature = "sys")]
pub(crate) use crate::sys::engine::default_engine;

#[cfg(feature = "js")]
use crate::js::engine as engine_imp;
#[cfg(feature = "js")]
pub(crate) use crate::js::engine::default_engine;

/// The engine type
#[derive(Clone, Debug)]
pub struct Engine(pub(crate) engine_imp::Engine);

impl Engine {
    #[deprecated(
        since = "3.2.0",
        note = "engine.cloned() has been deprecated in favor of engine.clone()"
    )]
    /// Returns the [`Engine`].
    pub fn cloned(&self) -> Self {
        self.clone()
    }

    /// Returns the deterministic id of this engine
    pub fn deterministic_id(&self) -> &str {
        self.0.deterministic_id()
    }
}

impl AsEngineRef for Engine {
    fn as_engine_ref(&self) -> EngineRef {
        EngineRef { inner: self }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self(default_engine())
    }
}

impl<T: Into<engine_imp::Engine>> From<T> for Engine {
    fn from(t: T) -> Self {
        Self(t.into())
    }
}

/// A temporary handle to an [`Engine`]
/// EngineRef can be used to build a [`Module`][wasmer::Module]
/// It can be created directly with an [`Engine`]
/// Or from anything implementing [`AsEngineRef`]
/// like from [`Store`][wasmer::Store] typicaly.
pub struct EngineRef<'a> {
    /// The inner engine
    pub(crate) inner: &'a Engine,
}

impl<'a> EngineRef<'a> {
    /// Get inner [`Engine`]
    pub fn engine(&self) -> &Engine {
        self.inner
    }
    /// Create an EngineRef from an Engine
    pub fn new(engine: &'a Engine) -> Self {
        EngineRef { inner: engine }
    }
}

/// Helper trait for a value that is convertible to a [`EngineRef`].
pub trait AsEngineRef {
    /// Returns a `EngineRef` pointing to the underlying context.
    fn as_engine_ref(&self) -> EngineRef<'_>;
}

impl AsEngineRef for EngineRef<'_> {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef { inner: self.inner }
    }
}

impl<P> AsEngineRef for P
where
    P: Deref,
    P::Target: AsEngineRef,
{
    fn as_engine_ref(&self) -> EngineRef<'_> {
        (**self).as_engine_ref()
    }
}
