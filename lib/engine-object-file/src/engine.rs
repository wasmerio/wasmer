use crate::ObjectFileArtifact;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
#[cfg(feature = "compiler")]
use wasmer_compiler::Compiler;
use wasmer_compiler::{CompileError, Target};
use wasmer_engine::{Artifact, DeserializeError, Engine, EngineId, Tunables};
#[cfg(feature = "compiler")]
use wasmer_types::Features;
use wasmer_types::FunctionType;
use wasmer_vm::{SignatureRegistry, VMSharedSignatureIndex, VMTrampoline};

/// A WebAssembly `ObjectFile` Engine.
#[derive(Clone)]
pub struct ObjectFileEngine {
    inner: Arc<Mutex<ObjectFileEngineInner>>,
    /// The target for the compiler
    target: Arc<Target>,
    engine_id: EngineId,
}

impl ObjectFileEngine {
    /// Create a new `ObjectFileEngine` with the given config
    #[cfg(feature = "compiler")]
    pub fn new(compiler: Box<dyn Compiler + Send>, target: Target, features: Features) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ObjectFileEngineInner {
                compiler: Some(compiler),
                trampolines: HashMap::new(),
                signatures: SignatureRegistry::new(),
                prefixer: None,
                features,
            })),
            target: Arc::new(target),
            engine_id: EngineId::default(),
        }
    }

    /// Create a headless `ObjectFileEngine`
    ///
    /// A headless engine is an engine without any compiler attached.
    /// This is useful for assuring a minimal runtime for running
    /// WebAssembly modules.
    ///
    /// For example, for running in IoT devices where compilers are very
    /// expensive, or also to optimize startup speed.
    ///
    /// # Important
    ///
    /// Headless engines can't compile or validate any modules,
    /// they just take already processed Modules (via `Module::serialize`).
    pub fn headless() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ObjectFileEngineInner {
                #[cfg(feature = "compiler")]
                compiler: None,
                #[cfg(feature = "compiler")]
                features: Features::default(),
                trampolines: HashMap::new(),
                signatures: SignatureRegistry::new(),
                prefixer: None,
            })),
            target: Arc::new(Target::default()),
            engine_id: EngineId::default(),
        }
    }

    /// Sets a prefixer for the wasm module, so we can avoid any collisions
    /// in the exported function names on the generated shared object.
    ///
    /// This, allows us to rather than have functions named `wasmer_function_1`
    /// to be named `wasmer_function_PREFIX_1`.
    ///
    /// # Important
    ///
    /// This prefixer function should be deterministic, so the compilation
    /// remains deterministic.
    pub fn set_deterministic_prefixer<F>(&mut self, prefixer: F)
    where
        F: Fn(&[u8]) -> String + Send + 'static,
    {
        let mut inner = self.inner_mut();
        inner.prefixer = Some(Box::new(prefixer));
    }

    pub(crate) fn inner(&self) -> std::sync::MutexGuard<'_, ObjectFileEngineInner> {
        self.inner.lock().unwrap()
    }

    pub(crate) fn inner_mut(&self) -> std::sync::MutexGuard<'_, ObjectFileEngineInner> {
        self.inner.lock().unwrap()
    }
}

impl Engine for ObjectFileEngine {
    /// The target
    fn target(&self) -> &Target {
        &self.target
    }

    /// Register a signature
    fn register_signature(&self, func_type: &FunctionType) -> VMSharedSignatureIndex {
        let compiler = self.inner();
        compiler.signatures().register(func_type)
    }

    /// Lookup a signature
    fn lookup_signature(&self, sig: VMSharedSignatureIndex) -> Option<FunctionType> {
        let compiler = self.inner();
        compiler.signatures().lookup(sig)
    }

    /*
    /// Retrieves a trampoline given a signature
    fn function_call_trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        self.inner().trampoline(sig)
    }
    */

    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        self.inner().validate(binary)
    }

    /// Compile a WebAssembly binary
    #[cfg(feature = "compiler")]
    fn compile(
        &self,
        binary: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Arc<dyn Artifact>, CompileError> {
        Ok(Arc::new(ObjectFileArtifact::new(&self, binary, tunables)?))
    }

    /// Compile a WebAssembly binary (it will fail because the `compiler` flag is disabled).
    #[cfg(not(feature = "compiler"))]
    fn compile(
        &self,
        _binary: &[u8],
        _tunables: &dyn Tunables,
    ) -> Result<Arc<dyn Artifact>, CompileError> {
        Err(CompileError::Codegen(
            "The `ObjectFileEngine` is operating in headless mode, so it cannot compile a module."
                .to_string(),
        ))
    }

    /// Deserializes a WebAssembly module (binary content of a Shared Object file)
    unsafe fn deserialize(&self, bytes: &[u8]) -> Result<Arc<dyn Artifact>, DeserializeError> {
        Ok(Arc::new(ObjectFileArtifact::deserialize(&self, &bytes)?))
    }

    /// Deserializes a WebAssembly module from a path
    /// It should point to a Shared Object file generated by this engine.
    unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<dyn Artifact>, DeserializeError> {
        let mut f = std::fs::File::open(file_ref)?;
        let mut vec = vec![];
        f.read_to_end(&mut vec)?;

        self.deserialize(&vec[..])
    }

    fn id(&self) -> &EngineId {
        &self.engine_id
    }

    fn cloned(&self) -> Arc<dyn Engine + Send + Sync> {
        Arc::new(self.clone())
    }
}

/// The inner contents of `ObjectFileEngine`
pub struct ObjectFileEngineInner {
    /// The compiler
    #[cfg(feature = "compiler")]
    compiler: Option<Box<dyn Compiler + Send>>,
    /// The WebAssembly features to use
    #[cfg(feature = "compiler")]
    features: Features,
    /// Pointers to trampoline functions used to enter particular signatures
    trampolines: HashMap<VMSharedSignatureIndex, VMTrampoline>,
    /// The signature registry is used mainly to operate with trampolines
    /// performantly.
    signatures: SignatureRegistry,
    /// The prefixer returns the a String to prefix each of
    /// the functions in the shared object generated by the `ObjectFileEngine`,
    /// so we can assure no collisions.
    prefixer: Option<Box<dyn Fn(&[u8]) -> String + Send>>,
}

impl ObjectFileEngineInner {
    /// Gets the compiler associated to this engine.
    #[cfg(feature = "compiler")]
    pub fn compiler(&self) -> Result<&dyn Compiler, CompileError> {
        if self.compiler.is_none() {
            return Err(CompileError::Codegen("The `ObjectFileEngine` is operating in headless mode, so it can only execute already compiled Modules.".to_string()));
        }
        Ok(&**self
            .compiler
            .as_ref()
            .expect("Can't get compiler reference"))
    }

    #[cfg(feature = "compiler")]
    pub(crate) fn get_prefix(&self, bytes: &[u8]) -> String {
        if let Some(prefixer) = &self.prefixer {
            prefixer(&bytes)
        } else {
            "".to_string()
        }
    }

    #[cfg(feature = "compiler")]
    pub(crate) fn features(&self) -> &Features {
        &self.features
    }

    /// Validate the module
    #[cfg(feature = "compiler")]
    pub fn validate<'data>(&self, data: &'data [u8]) -> Result<(), CompileError> {
        self.compiler()?.validate_module(self.features(), data)
    }

    /// Validate the module
    #[cfg(not(feature = "compiler"))]
    pub fn validate<'data>(&self, _data: &'data [u8]) -> Result<(), CompileError> {
        Err(CompileError::Validate(
            "The `ObjectFileEngine` is not compiled with compiler support, which is required for validating".to_string(),
        ))
    }

    /// Shared signature registry.
    pub fn signatures(&self) -> &SignatureRegistry {
        &self.signatures
    }

    /// Gets the trampoline pre-registered for a particular signature
    pub fn trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        self.trampolines.get(&sig).cloned()
    }

    pub(crate) fn add_trampoline(&mut self, func_type: &FunctionType, trampoline: VMTrampoline) {
        let index = self.signatures.register(&func_type);
        // We always use (for now) the latest trampoline compiled
        // TODO: we need to deallocate trampolines as the compiled modules
        // where they belong become unallocated.
        self.trampolines.insert(index, trampoline);
    }
}
