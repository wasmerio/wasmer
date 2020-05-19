//! Native Engine.

use crate::NativeModule;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use tempfile::NamedTempFile;
use wasm_common::FunctionType;
use wasmer_compiler::CompileError;
#[cfg(feature = "compiler")]
use wasmer_compiler::{Compiler, CompilerConfig};
use wasmer_engine::{
    CompiledModule as BaseCompiledModule, DeserializeError, Engine, InstantiationError, Resolver,
    SerializeError, Tunables,
};
use wasmer_runtime::{InstanceHandle, SignatureRegistry, VMSharedSignatureIndex, VMTrampoline};

/// A WebAssembly `Native` Engine.
#[derive(Clone)]
pub struct NativeEngine {
    inner: Arc<Mutex<NativeEngineInner>>,
    tunables: Arc<dyn Tunables + Send + Sync>,
}

impl NativeEngine {
    // Mach-O header in Mac
    #[allow(dead_code)]
    const MAGIC_HEADER_MH_CIGAM_64: &'static [u8] = &[207, 250, 237, 254];

    // ELF Magic header for Linux (32 bit)
    #[allow(dead_code)]
    const MAGIC_HEADER_ELF_32: &'static [u8] = &[0x7f, b'E', b'L', b'F', 1];

    // ELF Magic header for Linux (64 bit)
    #[allow(dead_code)]
    const MAGIC_HEADER_ELF_64: &'static [u8] = &[0x7f, b'E', b'L', b'F', 2];

    /// Create a new `NativeEngine` with the given config
    #[cfg(feature = "compiler")]
    pub fn new(
        mut config: Box<dyn CompilerConfig>,
        tunables: impl Tunables + 'static + Send + Sync,
    ) -> Self {
        config.enable_pic();
        let compiler = config.compiler();
        Self {
            inner: Arc::new(Mutex::new(NativeEngineInner {
                compiler: Some(compiler),
                trampolines: HashMap::new(),
                signatures: SignatureRegistry::new(),
                prefixer: None,
            })),
            tunables: Arc::new(tunables),
        }
    }

    /// Create a headless `NativeEngine`
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
    pub fn headless(tunables: impl Tunables + 'static + Send + Sync) -> Self {
        Self {
            inner: Arc::new(Mutex::new(NativeEngineInner {
                #[cfg(feature = "compiler")]
                compiler: None,
                trampolines: HashMap::new(),
                signatures: SignatureRegistry::new(),
                prefixer: None,
            })),
            tunables: Arc::new(tunables),
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

    pub(crate) fn inner(&self) -> std::sync::MutexGuard<'_, NativeEngineInner> {
        self.inner.lock().unwrap()
    }

    pub(crate) fn inner_mut(&self) -> std::sync::MutexGuard<'_, NativeEngineInner> {
        self.inner.lock().unwrap()
    }

    /// Check if the provided bytes look like a serialized
    /// module by the `Native` implementation.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        cfg_if::cfg_if! {
            if #[cfg(all(target_pointer_width = "64", target_os="macos"))] {
                return &bytes[..Self::MAGIC_HEADER_MH_CIGAM_64.len()] == Self::MAGIC_HEADER_MH_CIGAM_64;
            }
            else if #[cfg(all(target_pointer_width = "64", target_os="linux"))] {
                return &bytes[..Self::MAGIC_HEADER_ELF_64.len()] == Self::MAGIC_HEADER_ELF_64;
            }
            else if #[cfg(all(target_pointer_width = "32", target_os="linux"))] {
                return &bytes[..Self::MAGIC_HEADER_ELF_32.len()] == Self::MAGIC_HEADER_ELF_32;
            }
            else {
                false
            }
        }
    }
}

impl Engine for NativeEngine {
    /// Get the tunables
    fn tunables(&self) -> &dyn Tunables {
        &*self.tunables
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

    /// Retrieves a trampoline given a signature
    fn function_call_trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        self.inner().trampoline(sig)
    }

    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        self.inner().validate(binary)
    }

    /// Compile a WebAssembly binary
    fn compile(&self, binary: &[u8]) -> Result<Arc<dyn BaseCompiledModule>, CompileError> {
        Ok(Arc::new(NativeModule::new(&self, binary)?))
    }

    /// Instantiates a WebAssembly module
    unsafe fn instantiate(
        &self,
        compiled_module: &dyn BaseCompiledModule,
        resolver: &dyn Resolver,
    ) -> Result<InstanceHandle, InstantiationError> {
        let compiled_module = compiled_module
            .downcast_ref::<NativeModule>()
            .expect("The provided module is not a Native compiled module");
        compiled_module.instantiate(&self, resolver, Box::new(()))
    }

    /// Finish the instantiation of a WebAssembly module
    unsafe fn finish_instantiation(
        &self,
        compiled_module: &dyn BaseCompiledModule,
        handle: &InstanceHandle,
    ) -> Result<(), InstantiationError> {
        let compiled_module = compiled_module
            .downcast_ref::<NativeModule>()
            .expect("The provided module is not a Native compiled module");
        compiled_module.finish_instantiation(&handle)
    }

    /// Serializes a WebAssembly module
    fn serialize(
        &self,
        compiled_module: &dyn BaseCompiledModule,
    ) -> Result<Vec<u8>, SerializeError> {
        let compiled_module = compiled_module
            .downcast_ref::<NativeModule>()
            .expect("The provided module is not a Native compiled module");
        let serialized = compiled_module.serialize()?;
        Ok(serialized)
    }

    /// Deserializes a WebAssembly module (binary content of a Shared Object file)
    fn deserialize(&self, bytes: &[u8]) -> Result<Arc<dyn BaseCompiledModule>, DeserializeError> {
        if !Self::is_deserializable(&bytes) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not in any native format Wasmer can understand".to_string(),
            ));
        }
        // Dump the bytes into a file, so we can read it with our `dlopen`
        let named_file = NamedTempFile::new()?;
        let (mut file, path) = named_file.keep().map_err(|e| e.error)?;
        file.write_all(&bytes)?;
        self.deserialize_from_file(&path)
    }

    /// Deserializes a WebAssembly module from a path
    /// It should point to a Shared Object file generated by this engine.
    fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<dyn BaseCompiledModule>, DeserializeError> {
        let mut file = File::open(&file_ref)?;
        let mut buffer = [0; 5];
        // read up to 5 bytes
        file.read_exact(&mut buffer)?;
        if !Self::is_deserializable(&buffer) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not in any native format Wasmer can understand".to_string(),
            ));
        }
        unsafe {
            Ok(Arc::new(NativeModule::deserialize_from_file(
                &self, &file_ref,
            )?))
        }
    }
}

/// The inner contents of `NativeEngine`
pub struct NativeEngineInner {
    /// The compiler
    #[cfg(feature = "compiler")]
    compiler: Option<Box<dyn Compiler + Send>>,
    /// Pointers to trampoline functions used to enter particular signatures
    trampolines: HashMap<VMSharedSignatureIndex, VMTrampoline>,
    /// The signature registry is used mainly to operate with trampolines
    /// performantly.
    signatures: SignatureRegistry,
    /// The prefixer returns the a String to prefix each of
    /// the functions in the shared object generated by the `NativeEngine`,
    /// so we can assure no collisions.
    prefixer: Option<Box<dyn Fn(&[u8]) -> String + Send>>,
}

impl NativeEngineInner {
    /// Gets the compiler associated to this JIT
    #[cfg(feature = "compiler")]
    pub fn compiler(&self) -> Result<&dyn Compiler, CompileError> {
        if self.compiler.is_none() {
            return Err(CompileError::Codegen("The NativeEngine is operating in headless mode, so it can only execute already compiled Modules.".to_string()));
        }
        Ok(&**self
            .compiler
            .as_ref()
            .expect("Can't get compiler reference"))
    }

    pub(crate) fn get_prefix(&self, bytes: &[u8]) -> String {
        if let Some(prefixer) = &self.prefixer {
            prefixer(&bytes)
        } else {
            "".to_string()
        }
    }

    /// Validate the module
    #[cfg(feature = "compiler")]
    pub fn validate<'data>(&self, data: &'data [u8]) -> Result<(), CompileError> {
        self.compiler()?.validate_module(data)
    }

    /// Validate the module
    #[cfg(not(feature = "compiler"))]
    pub fn validate<'data>(&self, data: &'data [u8]) -> Result<(), CompileError> {
        Err(CompileError::Validate(
            "Validation is only enabled with the compiler feature".to_string(),
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
