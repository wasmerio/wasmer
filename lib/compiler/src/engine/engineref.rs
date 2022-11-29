use super::Engine;
use crate::Tunables;

/// A temporary handle to an [`Engine`]
/// EngineRef can be used to build a [`Module`][crate::sys::Module]
/// It can be created directly with an [`Engine`]
/// Or from anything implementing [`AsEngineRef`]
/// like from [`Store`][crate::sys::Store] typicaly
/// # Example
///
/// ```
/// # use crate::{Cranelift, EngineBuilder, BaseTunables, EngineRef, Module};
/// #
/// # let compiler = Cranelift::default();
/// # let engine = EngineBuilder::new(compiler).engine();
/// # let engineref = EngineRef::new(&engine);
///
/// let module = Module::from_file(&engineref, "path/to/foo.wasm");
/// ```
pub struct EngineRef<'a> {
    /// The inner engine
    pub(crate) inner: &'a Engine,
}

impl<'a> EngineRef<'a> {
    /// Get inner [`Engine`]
    pub fn engine(&self) -> &Engine {
        self.inner
    }
    /// Get the [`Tunables`]
    pub fn tunables(&self) -> &dyn Tunables {
        self.inner.tunables()
    }
    /// Create an EngineRef from an Engine and Tunables
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
