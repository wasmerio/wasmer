//! JIT compilation.

use crate::unwind::UnwindRegistry;
use crate::{CodeMemory, JITArtifact};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasm_common::entity::PrimaryMap;
use wasm_common::Features;
use wasm_common::{FunctionIndex, FunctionType, LocalFunctionIndex, SignatureIndex};
#[cfg(feature = "compiler")]
use wasmer_compiler::Compiler;
use wasmer_compiler::{
    CompileError, CustomSection, CustomSectionProtection, FunctionBody, SectionIndex, Target,
};
use wasmer_engine::{Artifact, DeserializeError, Engine, EngineId, Tunables};
use wasmer_vm::{
    FunctionBodyPtr, ModuleInfo, SignatureRegistry, VMFunctionBody, VMSharedSignatureIndex,
    VMTrampoline,
};

/// A WebAssembly `JIT` Engine.
#[derive(Clone)]
pub struct JITEngine {
    inner: Arc<Mutex<JITEngineInner>>,
    /// The target for the compiler
    target: Arc<Target>,
    engine_id: EngineId,
}

impl JITEngine {
    /// Create a new `JITEngine` with the given config
    #[cfg(feature = "compiler")]
    pub fn new(compiler: Box<dyn Compiler + Send>, target: Target, features: Features) -> Self {
        Self {
            inner: Arc::new(Mutex::new(JITEngineInner {
                compiler: Some(compiler),
                function_call_trampolines: HashMap::new(),
                code_memory: CodeMemory::new(),
                signatures: SignatureRegistry::new(),
                features,
            })),
            target: Arc::new(target),
            engine_id: EngineId::default(),
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
    pub fn headless() -> Self {
        Self {
            inner: Arc::new(Mutex::new(JITEngineInner {
                #[cfg(feature = "compiler")]
                compiler: None,
                function_call_trampolines: HashMap::new(),
                code_memory: CodeMemory::new(),
                signatures: SignatureRegistry::new(),
                features: Features::default(),
            })),
            target: Arc::new(Target::default()),
            engine_id: EngineId::default(),
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

    /// Retrieves a trampoline given a signature
    fn function_call_trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        self.inner().function_call_trampoline(sig)
    }

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
        Ok(Arc::new(JITArtifact::new(&self, binary, tunables)?))
    }

    /// Compile a WebAssembly binary
    #[cfg(not(feature = "compiler"))]
    fn compile(
        &self,
        _binary: &[u8],
        _tunables: &dyn Tunables,
    ) -> Result<Arc<dyn Artifact>, CompileError> {
        Err(CompileError::Codegen(
            "The JITEngine is operating in headless mode, so it can not compile Modules."
                .to_string(),
        ))
    }

    /// Deserializes a WebAssembly module
    unsafe fn deserialize(&self, bytes: &[u8]) -> Result<Arc<dyn Artifact>, DeserializeError> {
        Ok(Arc::new(JITArtifact::deserialize(&self, &bytes)?))
    }

    fn id(&self) -> &EngineId {
        &self.engine_id
    }

    fn cloned(&self) -> Arc<dyn Engine + Send + Sync> {
        Arc::new(self.clone())
    }
}

/// The inner contents of `JITEngine`
pub struct JITEngineInner {
    /// The compiler
    #[cfg(feature = "compiler")]
    compiler: Option<Box<dyn Compiler + Send>>,
    /// Pointers to trampoline functions used to enter particular signatures
    function_call_trampolines: HashMap<VMSharedSignatureIndex, VMTrampoline>,
    /// The features to compile the Wasm module with
    features: Features,
    /// The code memory is responsible of publishing the compiled
    /// functions to memory.
    code_memory: CodeMemory,
    /// The signature registry is used mainly to operate with trampolines
    /// performantly.
    signatures: SignatureRegistry,
}

impl JITEngineInner {
    /// Gets the compiler associated to this engine.
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
        self.compiler()?.validate_module(self.features(), data)
    }

    /// Validate the module
    #[cfg(not(feature = "compiler"))]
    pub fn validate<'data>(&self, _data: &'data [u8]) -> Result<(), CompileError> {
        Err(CompileError::Validate(
            "The JITEngine is not compiled with compiler support, which is required for validating"
                .to_string(),
        ))
    }

    /// The Wasm features
    pub fn features(&self) -> &Features {
        &self.features
    }

    /// Allocate custom sections into memory
    pub(crate) fn allocate_custom_sections(
        &mut self,
        custom_sections: &PrimaryMap<SectionIndex, CustomSection>,
    ) -> Result<PrimaryMap<SectionIndex, *const u8>, CompileError> {
        let mut result = PrimaryMap::with_capacity(custom_sections.len());
        for (_, section) in custom_sections.iter() {
            let buffer: &[u8] = match section.protection {
                CustomSectionProtection::Read => self
                    .code_memory
                    .allocate_for_custom_section(&section.bytes)
                    .map_err(|message| {
                        CompileError::Resource(format!(
                            "failed to allocate readable memory for custom section: {}",
                            message
                        ))
                    })?,
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
    #[allow(clippy::type_complexity)]
    pub(crate) fn allocate(
        &mut self,
        registry: &mut UnwindRegistry,
        module: &ModuleInfo,
        functions: &PrimaryMap<LocalFunctionIndex, FunctionBody>,
        function_call_trampolines: &PrimaryMap<SignatureIndex, FunctionBody>,
        dynamic_function_trampolines: &PrimaryMap<FunctionIndex, FunctionBody>,
    ) -> Result<
        (
            PrimaryMap<LocalFunctionIndex, FunctionBodyPtr>,
            PrimaryMap<SignatureIndex, FunctionBodyPtr>,
            PrimaryMap<FunctionIndex, FunctionBodyPtr>,
        ),
        CompileError,
    > {
        // Allocate all of the compiled functions into executable memory,
        // copying over their contents.
        let allocated_functions = self
            .code_memory
            .allocate_functions(registry, &functions)
            .map_err(|message| {
                CompileError::Resource(format!(
                    "failed to allocate memory for functions: {}",
                    message
                ))
            })?;

        let mut alllocated_function_call_trampolines: PrimaryMap<SignatureIndex, FunctionBodyPtr> =
            PrimaryMap::new();
        // let (indices, compiled_functions): (Vec<VMSharedSignatureIndex>, PrimaryMap<FunctionIndex, FunctionBody>) = function_call_trampolines.iter().map(|(sig_index, compiled_function)| {
        //     let func_type = module.signatures.get(sig_index).unwrap();
        //     let index = self.signatures.register(&func_type);
        //     (index, compiled_function)
        // }).filter(|(index, _)| {
        //     !self.function_call_trampolines.contains_key(index)
        // }).unzip();
        for (sig_index, compiled_function) in function_call_trampolines.iter() {
            let func_type = module.signatures.get(sig_index).unwrap();
            let index = self.signatures.register(&func_type);
            // if self.function_call_trampolines.contains_key(&index) {
            //     // We don't need to allocate the trampoline in case
            //     // it's signature is already allocated.
            //     continue;
            // }
            let ptr = self
                .code_memory
                .allocate_for_function(registry, &compiled_function)
                .map_err(|message| {
                    CompileError::Resource(format!(
                        "failed to allocate memory for function call trampolines: {}",
                        message
                    ))
                })?;
            alllocated_function_call_trampolines.push(FunctionBodyPtr(ptr));
            let trampoline =
                unsafe { std::mem::transmute::<*const VMFunctionBody, VMTrampoline>(ptr.as_ptr()) };
            self.function_call_trampolines.insert(index, trampoline);
        }

        let allocated_dynamic_function_trampolines = dynamic_function_trampolines
            .values()
            .map(|compiled_function| {
                let ptr = self
                    .code_memory
                    .allocate_for_function(registry, &compiled_function)
                    .map_err(|message| {
                        CompileError::Resource(format!(
                            "failed to allocate memory for dynamic function trampolines: {}",
                            message
                        ))
                    })?;
                Ok(FunctionBodyPtr(ptr as _))
            })
            .collect::<Result<PrimaryMap<FunctionIndex, _>, CompileError>>()?;

        Ok((
            allocated_functions,
            alllocated_function_call_trampolines,
            allocated_dynamic_function_trampolines,
        ))
    }

    /// Make memory containing compiled code executable.
    pub(crate) fn publish_compiled_code(&mut self) {
        self.code_memory.publish();
    }

    /// Publish the unwind registry into code memory.
    pub(crate) fn publish_unwind_registry(&mut self, unwind_registry: Arc<UnwindRegistry>) {
        self.code_memory.publish_unwind_registry(unwind_registry);
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
