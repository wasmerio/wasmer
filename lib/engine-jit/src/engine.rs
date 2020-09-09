//! JIT compilation.

use crate::unwind::UnwindRegistry;
use crate::{CodeMemory, JITArtifact};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
#[cfg(feature = "compiler")]
use wasmer_compiler::Compiler;
use wasmer_compiler::{
    CompileError, CustomSection, CustomSectionProtection, FunctionBody, SectionBody, SectionIndex,
    Target,
};
use wasmer_engine::{Artifact, DeserializeError, Engine, EngineId, Tunables};
use wasmer_types::entity::PrimaryMap;
use wasmer_types::Features;
use wasmer_types::{FunctionIndex, FunctionType, LocalFunctionIndex, SignatureIndex};
use wasmer_vm::{
    FunctionBodyPtr, ModuleInfo, SectionBodyPtr, SignatureRegistry, VMFunctionBody,
    VMSharedSignatureIndex, VMTrampoline,
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
                code_memory: vec![],
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
                code_memory: vec![],
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
    code_memory: Vec<CodeMemory>,
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

    /// Allocate compiled functions into memory
    #[allow(clippy::type_complexity)]
    pub(crate) fn allocate(
        &mut self,
        registry: &mut UnwindRegistry,
        module: &ModuleInfo,
        functions: &PrimaryMap<LocalFunctionIndex, FunctionBody>,
        function_call_trampolines: &PrimaryMap<SignatureIndex, FunctionBody>,
        dynamic_function_trampolines: &PrimaryMap<FunctionIndex, FunctionBody>,
        custom_sections: &PrimaryMap<SectionIndex, CustomSection>,
    ) -> Result<
        (
            PrimaryMap<LocalFunctionIndex, FunctionBodyPtr>,
            PrimaryMap<SignatureIndex, FunctionBodyPtr>,
            PrimaryMap<FunctionIndex, FunctionBodyPtr>,
            PrimaryMap<SectionIndex, SectionBodyPtr>,
        ),
        CompileError,
    > {
        let function_bodies = functions
            .values()
            .chain(function_call_trampolines.values())
            .chain(dynamic_function_trampolines.values())
            .collect::<Vec<_>>();
        let (executable_sections, data_sections): (Vec<_>, _) = custom_sections
            .values()
            .partition(|section| section.protection == CustomSectionProtection::ReadExecute);

        self.code_memory.push(CodeMemory::new());

        let (mut allocated_functions, allocated_executable_sections, allocated_data_sections) =
            self.code_memory
                .last_mut()
                .unwrap()
                .allocate(
                    registry,
                    function_bodies.as_slice(),
                    executable_sections.as_slice(),
                    data_sections.as_slice(),
                )
                .map_err(|message| {
                    CompileError::Resource(format!(
                        "failed to allocate memory for functions: {}",
                        message
                    ))
                })?;

        let allocated_functions_result = allocated_functions
            .drain(0..functions.len())
            .map(|slice| FunctionBodyPtr(slice as *mut [_]))
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let mut allocated_function_call_trampolines: PrimaryMap<SignatureIndex, FunctionBodyPtr> =
            PrimaryMap::new();
        for (i, (sig_index, compiled_function)) in function_call_trampolines.iter().enumerate() {
            let func_type = module.signatures.get(sig_index).unwrap();
            let index = self.signatures.register(&func_type);
            let ptr = allocated_functions
                .drain(0..1)
                .map(|slice| FunctionBodyPtr(slice as *mut [_]))
                .collect::<Vec<_>>()[0];
            allocated_function_call_trampolines.push(ptr);
            let trampoline = unsafe {
                std::mem::transmute::<*const VMFunctionBody, VMTrampoline>((**ptr).as_ptr())
            };
            self.function_call_trampolines.insert(index, trampoline);
        }

        let allocated_dynamic_function_trampolines = allocated_functions
            .drain(..)
            .map(|slice| FunctionBodyPtr(slice as *mut [_]))
            .collect::<PrimaryMap<FunctionIndex, _>>();

        let mut exec_iter = allocated_executable_sections.iter();
        let mut data_iter = allocated_data_sections.iter();
        let allocated_custom_sections = custom_sections
            .iter()
            .map(|(_, section)| {
                SectionBodyPtr(
                    if section.protection == CustomSectionProtection::ReadExecute {
                        exec_iter.next()
                    } else {
                        data_iter.next()
                    }
                    .unwrap()
                    .as_ptr(),
                )
            })
            .collect::<PrimaryMap<SectionIndex, _>>();

        Ok((
            allocated_functions_result,
            allocated_function_call_trampolines,
            allocated_dynamic_function_trampolines,
            allocated_custom_sections,
        ))
    }

    /// Make memory containing compiled code executable.
    pub(crate) fn publish_compiled_code(&mut self) {
        self.code_memory.last_mut().unwrap().publish();
    }

    /// Publish the unwind registry into code memory.
    pub(crate) fn publish_unwind_registry(&mut self, unwind_registry: Arc<UnwindRegistry>) {
        self.code_memory
            .last_mut()
            .unwrap()
            .publish_unwind_registry(unwind_registry);
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
