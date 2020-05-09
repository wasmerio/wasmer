//! Native Engine.

use crate::NativeModule;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use tempfile::NamedTempFile;
use wasm_common::entity::PrimaryMap;
use wasm_common::{FunctionType, LocalFunctionIndex, MemoryIndex, SignatureIndex, TableIndex};
use wasmer_compiler::{Compilation, CompileError, FunctionBody, Target};
#[cfg(feature = "compiler")]
use wasmer_compiler::{Compiler, CompilerConfig};
use wasmer_engine::{
    CompiledModule as BaseCompiledModule, DeserializeError, Engine, InstantiationError, Resolver,
    SerializeError, Tunables,
};
use wasmer_runtime::{
    InstanceHandle, MemoryPlan, Module, SignatureRegistry, TablePlan, VMFunctionBody,
    VMSharedSignatureIndex, VMTrampoline,
};

/// A WebAssembly `Native` Engine.
#[derive(Clone)]
pub struct NativeEngine {
    inner: Arc<Mutex<NativeEngineInner>>,
    tunables: Arc<dyn Tunables + Send + Sync>,
}

impl NativeEngine {
    // Mach-O header in Mac
    const MAGIC_HEADER_MH_CIGAM_64: &'static [u8] = &[207, 250, 237, 254];

    // ELF Magic header for Linux (32 bit)
    const MAGIC_HEADER_ELF_32: &'static [u8] = &[0x7f, b'E', b'L', b'F', 0];

    // ELF Magic header for Linux (64 bit)
    const MAGIC_HEADER_ELF_64: &'static [u8] = &[0x7f, b'E', b'L', b'F', 1];

    /// Create a new `NativeEngine` with the given config
    #[cfg(feature = "compiler")]
    pub fn new<C: CompilerConfig>(
        config: &C,
        tunables: impl Tunables + 'static + Send + Sync,
    ) -> Self
    where
        C: ?Sized,
    {
        let compiler = config.compiler();
        Self {
            inner: Arc::new(Mutex::new(NativeEngineInner {
                compiler: Some(compiler),
                trampolines: HashMap::new(),
                signatures: SignatureRegistry::new(),
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
            })),
            tunables: Arc::new(tunables),
        }
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
                return &bytes[..4] == Self::MAGIC_HEADER_MH_CIGAM_64;
            }
            else if #[cfg(all(target_pointer_width = "64", target_os="linux"))] {
                return &bytes[..5] == Self::MAGIC_HEADER_ELF_64;
            }
            else if #[cfg(all(target_pointer_width = "32", target_os="linux"))] {
                return &bytes[..5] == Self::MAGIC_HEADER_ELF_32;
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
    fn trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
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
        let compiled_module = compiled_module.downcast_ref::<NativeModule>().unwrap();
        unsafe { compiled_module.instantiate(&self, resolver, Box::new(())) }
    }

    /// Finish the instantiation of a WebAssembly module
    unsafe fn finish_instantiation(
        &self,
        compiled_module: &dyn BaseCompiledModule,
        handle: &InstanceHandle,
    ) -> Result<(), InstantiationError> {
        let compiled_module = compiled_module.downcast_ref::<NativeModule>().unwrap();
        unsafe { compiled_module.finish_instantiation(&handle) }
    }

    /// Serializes a WebAssembly module
    fn serialize(
        &self,
        compiled_module: &dyn BaseCompiledModule,
    ) -> Result<Vec<u8>, SerializeError> {
        let compiled_module = compiled_module.downcast_ref::<NativeModule>().unwrap();
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
        file.read(&mut buffer)?;
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
}

impl NativeEngineInner {
    /// Gets the compiler associated to this JIT
    #[cfg(feature = "compiler")]
    pub fn compiler(&self) -> Result<&dyn Compiler, CompileError> {
        if self.compiler.is_none() {
            return Err(CompileError::Codegen("The NativeEngine is operating in headless mode, so it can only execute already compiled Modules.".to_string()));
        }
        Ok(&**self.compiler.as_ref().unwrap())
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
        if self.trampolines.contains_key(&index) {
            // We don't need to allocate the trampoline in case
            // it's signature is already allocated.
            return;
        }
        self.trampolines.insert(index, trampoline);
    }
}
