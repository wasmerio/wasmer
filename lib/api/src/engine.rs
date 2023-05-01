use core::ops::Deref;

#[cfg(feature = "sys")]
use crate::sys::engine as engine_imp;
#[cfg(feature = "sys")]
pub(crate) use crate::sys::engine::default_engine;
#[cfg(feature = "sys")]
use std::io::Read;
#[cfg(feature = "sys")]
use std::path::Path;
#[cfg(feature = "sys")]
use std::sync::Arc;
#[cfg(feature = "sys")]
pub use wasmer_compiler::{Artifact, CompilerConfig, EngineInner, Features, Tunables};
#[cfg(feature = "sys")]
use wasmer_types::{CompileError, DeserializeError, FunctionType, Target};
#[cfg(feature = "sys")]
use wasmer_vm::VMSharedSignatureIndex;

#[cfg(feature = "js")]
use crate::js::engine as engine_imp;
#[cfg(feature = "js")]
pub(crate) use crate::js::engine::default_engine;

#[cfg(feature = "jsc")]
use crate::jsc::engine as engine_imp;
#[cfg(feature = "jsc")]
pub(crate) use crate::jsc::engine::default_engine;
#[cfg(feature = "sys")]
type EngineId = str;

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

    #[deprecated(since = "3.2.0")]
    #[cfg(all(feature = "compiler", feature = "sys"))]
    /// Create a new `Engine` with the given config
    pub fn new(
        compiler_config: Box<dyn CompilerConfig>,
        target: Target,
        features: Features,
    ) -> Self {
        Self(engine_imp::Engine::new(compiler_config, target, features))
    }

    #[cfg(feature = "sys")]
    /// Create a headless `Engine`
    /// Will be removed in 4.0 in favor of the NativeEngineExt trait
    pub fn headless() -> Self {
        Self(engine_imp::Engine::headless())
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(feature = "sys")]
    /// Get reference to `EngineInner`.
    pub fn inner(&self) -> std::sync::MutexGuard<'_, EngineInner> {
        self.0.inner()
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(feature = "sys")]
    /// Get mutable reference to `EngineInner`.
    pub fn inner_mut(&self) -> std::sync::MutexGuard<'_, EngineInner> {
        self.0.inner_mut()
    }

    #[cfg(feature = "sys")]
    /// Gets the target
    /// Will be removed in 4.0 in favor of the NativeEngineExt trait
    pub fn target(&self) -> &Target {
        self.0.target()
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(feature = "sys")]
    /// Register a signature
    #[cfg(not(target_arch = "wasm32"))]
    pub fn register_signature(&self, func_type: &FunctionType) -> VMSharedSignatureIndex {
        let compiler = self.0.inner();
        compiler.signatures().register(func_type)
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(feature = "sys")]
    /// Lookup a signature
    #[cfg(not(target_arch = "wasm32"))]
    pub fn lookup_signature(&self, sig: VMSharedSignatureIndex) -> Option<FunctionType> {
        let compiler = self.0.inner();
        compiler.signatures().lookup(sig)
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(feature = "sys")]
    /// Validates a WebAssembly module
    #[cfg(feature = "compiler")]
    pub fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        self.0.inner().validate(binary)
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(all(feature = "sys", feature = "compiler"))]
    #[cfg(not(target_arch = "wasm32"))]
    /// Compile a WebAssembly binary
    pub fn compile(&self, binary: &[u8]) -> Result<Arc<Artifact>, CompileError> {
        Ok(Arc::new(Artifact::new(&self.0, binary, self.0.tunables())?))
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(all(feature = "sys", not(feature = "compiler")))]
    #[cfg(not(target_arch = "wasm32"))]
    /// Compile a WebAssembly binary
    pub fn compile(
        &self,
        _binary: &[u8],
        _tunables: &dyn Tunables,
    ) -> Result<Arc<Artifact>, CompileError> {
        Err(CompileError::Codegen(
            "The Engine is operating in headless mode, so it can not compile Modules.".to_string(),
        ))
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Deserializes a WebAssembly module
    ///
    /// # Safety
    ///
    /// The serialized content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize(&self, bytes: &[u8]) -> Result<Arc<Artifact>, DeserializeError> {
        Ok(Arc::new(Artifact::deserialize(&self.0, bytes)?))
    }

    #[deprecated(since = "3.2.0")]
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Deserializes a WebAssembly module from a path
    ///
    /// # Safety
    ///
    /// The file's content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        let mut file = std::fs::File::open(file_ref)?;
        let mut buffer = Vec::new();
        // read the whole file
        file.read_to_end(&mut buffer)?;
        Ok(Arc::new(Artifact::deserialize(&self.0, buffer.as_slice())?))
    }

    #[deprecated(since = "3.2.0", note = "Use Engine::deterministic_id()")]
    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// A unique identifier for this object.
    ///
    /// This exists to allow us to compare two Engines for equality. Otherwise,
    /// comparing two trait objects unsafely relies on implementation details
    /// of trait representation.
    pub fn id(&self) -> &EngineId {
        self.deterministic_id()
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Attach a Tunable to this engine
    /// Will be removed in 4.0 in favor of the NativeEngineExt trait
    pub fn set_tunables(&mut self, tunables: impl Tunables + Send + Sync + 'static) {
        self.0.set_tunables(tunables);
    }

    #[cfg(all(feature = "sys", not(target_arch = "wasm32")))]
    /// Get a reference to attached Tunable of this engine
    /// Will be removed in 4.0 in favor of the NativeEngineExt trait
    pub fn tunables(&self) -> &dyn Tunables {
        self.0.tunables()
    }
}
impl AsEngineRef for Engine {
    #[inline]
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
/// EngineRef can be used to build a [`Module`][super::Module]
/// It can be created directly with an [`Engine`]
/// Or from anything implementing [`AsEngineRef`]
/// like from [`Store`][super::Store] typicaly.
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
    #[inline]
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef { inner: self.inner }
    }
}

impl<P> AsEngineRef for P
where
    P: Deref,
    P::Target: AsEngineRef,
{
    #[inline]
    fn as_engine_ref(&self) -> EngineRef<'_> {
        (**self).as_engine_ref()
    }
}
