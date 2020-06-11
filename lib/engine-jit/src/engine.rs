//! JIT compilation.

use crate::{CodeMemory, JITArtifact};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasm_common::entity::PrimaryMap;
use wasm_common::{FunctionIndex, FunctionType, LocalFunctionIndex, SignatureIndex};
use wasmer_compiler::{
    CompileError, CustomSection, CustomSectionProtection, FunctionBody, SectionIndex,
};
#[cfg(feature = "compiler")]
use wasmer_compiler::{Compiler, CompilerConfig};
use wasmer_engine::{Artifact, DeserializeError, Engine, Tunables};
use wasmer_runtime::{
    ModuleInfo, SignatureRegistry, VMFunctionBody, VMSharedSignatureIndex, VMTrampoline,
};

/// A WebAssembly `JIT` Engine.
#[derive(Clone)]
pub struct JITEngine {
    inner: Arc<Mutex<JITEngineInner>>,
    tunables: Arc<dyn Tunables + Send + Sync>,
}

impl JITEngine {
    /// Create a new `JITEngine` with the given config
    #[cfg(feature = "compiler")]
    pub fn new(
        config: Box<dyn CompilerConfig>,
        tunables: impl Tunables + 'static + Send + Sync,
    ) -> Self {
        let compiler = config.compiler();
        Self {
            inner: Arc::new(Mutex::new(JITEngineInner {
                compiler: Some(compiler),
                function_call_trampolines: HashMap::new(),
                code_memory: CodeMemory::new(),
                signatures: SignatureRegistry::new(),
            })),
            tunables: Arc::new(tunables),
        }
    }

    /// Create a headless `JITEngine`
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
            inner: Arc::new(Mutex::new(JITEngineInner {
                #[cfg(feature = "compiler")]
                compiler: None,
                function_call_trampolines: HashMap::new(),
                code_memory: CodeMemory::new(),
                signatures: SignatureRegistry::new(),
            })),
            tunables: Arc::new(tunables),
        }
    }

    pub(crate) fn inner(&self) -> std::sync::MutexGuard<'_, JITEngineInner> {
        self.inner.lock().unwrap()
    }

    pub(crate) fn inner_mut(&self) -> std::sync::MutexGuard<'_, JITEngineInner> {
        self.inner.lock().unwrap()
    }
}

impl Engine for JITEngine {
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
        self.inner().function_call_trampoline(sig)
    }

    /// Validates a WebAssembly module
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        self.inner().validate(binary)
    }

    /// Compile a WebAssembly binary
    fn compile(&self, binary: &[u8]) -> Result<Arc<dyn Artifact>, CompileError> {
        Ok(Arc::new(JITArtifact::new(&self, binary)?))
    }

    /// Deserializes a WebAssembly module
    unsafe fn deserialize(&self, bytes: &[u8]) -> Result<Arc<dyn Artifact>, DeserializeError> {
        Ok(Arc::new(JITArtifact::deserialize(&self, &bytes)?))
    }
}

/// The inner contents of `JITEngine`
pub struct JITEngineInner {
    /// The compiler
    #[cfg(feature = "compiler")]
    compiler: Option<Box<dyn Compiler + Send>>,
    /// Pointers to trampoline functions used to enter particular signatures
    function_call_trampolines: HashMap<VMSharedSignatureIndex, VMTrampoline>,
    /// The code memory is responsible of publishing the compiled
    /// functions to memory.
    code_memory: CodeMemory,
    /// The signature registry is used mainly to operate with trampolines
    /// performantly.
    signatures: SignatureRegistry,
}

impl JITEngineInner {
    /// Gets the compiler associated to this JIT
    #[cfg(feature = "compiler")]
    pub fn compiler(&self) -> Result<&dyn Compiler, CompileError> {
        if self.compiler.is_none() {
            return Err(CompileError::Codegen("The JITEngine is operating in headless mode, so it can only execute already compiled Modules.".to_string()));
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

    /// Allocate custom sections into memory
    pub(crate) fn allocate_custom_sections(
        &mut self,
        custom_sections: &PrimaryMap<SectionIndex, CustomSection>,
    ) -> Result<PrimaryMap<SectionIndex, *const u8>, CompileError> {
        let mut result = PrimaryMap::with_capacity(custom_sections.len());
        for (_, section) in custom_sections.iter() {
            let buffer: &[u8] = match section.protection {
                CustomSectionProtection::Read => section.bytes.as_slice(),
                CustomSectionProtection::ReadExecute => self
                    .code_memory
                    .allocate_for_executable_custom_section(&section.bytes)
                    .map_err(|message| {
                        CompileError::Resource(format!(
                            "failed to allocate executable memory for custom section: {}",
                            message
                        ))
                    })?,
            };
            result.push(buffer.as_ptr());
        }
        Ok(result)
    }

    /// Allocate compiled functions into memory
    pub(crate) fn allocate(
        &mut self,
        module: &ModuleInfo,
        functions: &PrimaryMap<LocalFunctionIndex, FunctionBody>,
        function_call_trampolines: &PrimaryMap<SignatureIndex, FunctionBody>,
        dynamic_function_trampolines: &PrimaryMap<FunctionIndex, FunctionBody>,
    ) -> Result<
        (
            PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]>,
            PrimaryMap<FunctionIndex, *const VMFunctionBody>,
        ),
        CompileError,
    > {
        // Allocate all of the compiled functions into executable memory,
        // copying over their contents.
        let allocated_functions =
            self.code_memory
                .allocate_functions(&functions)
                .map_err(|message| {
                    CompileError::Resource(format!(
                        "failed to allocate memory for functions: {}",
                        message
                    ))
                })?;

        for (sig_index, compiled_function) in function_call_trampolines.iter() {
            let func_type = module.signatures.get(sig_index).unwrap();
            let index = self.signatures.register(&func_type);
            if self.function_call_trampolines.contains_key(&index) {
                // We don't need to allocate the trampoline in case
                // it's signature is already allocated.
                continue;
            }
            let ptr = self
                .code_memory
                .allocate_for_function(&compiled_function)
                .map_err(|message| {
                    CompileError::Resource(format!(
                        "failed to allocate memory for function call trampolines: {}",
                        message
                    ))
                })?
                .as_ptr();
            let trampoline =
                unsafe { std::mem::transmute::<*const VMFunctionBody, VMTrampoline>(ptr) };
            self.function_call_trampolines.insert(index, trampoline);
        }

        let allocated_dynamic_function_trampolines = dynamic_function_trampolines
            .values()
            .map(|compiled_function| {
                let ptr = self
                    .code_memory
                    .allocate_for_function(&compiled_function)
                    .map_err(|message| {
                        CompileError::Resource(format!(
                            "failed to allocate memory for dynamic function trampolines: {}",
                            message
                        ))
                    })?
                    .as_ptr();
                Ok(ptr)
            })
            .collect::<Result<PrimaryMap<FunctionIndex, _>, CompileError>>()?;

        Ok((allocated_functions, allocated_dynamic_function_trampolines))
    }

    /// Make memory containing compiled code executable.
    pub(crate) fn publish_compiled_code(&mut self, eh_frame: Option<&[u8]>) {
        self.code_memory.publish(eh_frame);
    }

    /// Shared signature registry.
    pub fn signatures(&self) -> &SignatureRegistry {
        &self.signatures
    }

    /// Gets the trampoline pre-registered for a particular signature
    pub fn function_call_trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        self.function_call_trampolines.get(&sig).cloned()
    }
}
