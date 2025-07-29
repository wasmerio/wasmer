use crate::entities::store::StoreRef;

use super::Engine;

/// A temporary handle to an [`Engine`].
///
/// An [`EngineRef`] can be used to build a [`Module`][crate::entities::Module], and can be created directly
/// from an [`Engine`] or from anything implementing [`AsEngineRef`], like a
/// [`Store`][crate::entities::Store].
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
    // TODO maybe this should be called `StoreObject`
    /// The type of objects associated with this engine seen as a store, if any.
    type Object;

    /// Create an [`EngineRef`] pointing to the underlying context.
    fn as_engine_ref(&self) -> EngineRef<'_>;

    /// Create a [`StoreRef`].
    ///
    /// NOTE: this function will return [`None`] if the [`AsEngineRef`] implementor is not an
    /// actual [`crate::Store`].
    fn maybe_as_store(&self) -> Option<StoreRef<'_, Self::Object>> {
        None
    }
}

impl AsEngineRef for EngineRef<'_> {
    type Object = std::convert::Infallible;

    #[inline]
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef { inner: self.inner }
    }
}

impl<P> AsEngineRef for P
where
    P: std::ops::Deref,
    P::Target: AsEngineRef,
{
    type Object = <P::Target as AsEngineRef>::Object;

    #[inline]
    fn as_engine_ref(&self) -> EngineRef<'_> {
        (**self).as_engine_ref()
    }

    fn maybe_as_store(&self) -> Option<StoreRef<'_, Self::Object>> {
        (**self).maybe_as_store()
    }
}

impl AsEngineRef for Engine {
    type Object = std::convert::Infallible;

    #[inline]
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef { inner: self }
    }
}
