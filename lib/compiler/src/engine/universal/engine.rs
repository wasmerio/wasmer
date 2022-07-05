//! Universal compilation.

#[cfg(feature = "universal_engine")]
use crate::Compiler;
use crate::EngineBuilder;
use crate::{Artifact, CodeMemory};
use crate::{FunctionExtent, Tunables};
use memmap2::Mmap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use wasmer_types::entity::PrimaryMap;
use wasmer_types::FunctionBody;
use wasmer_types::{
    CompileError, DeserializeError, Features, FunctionIndex, FunctionType, LocalFunctionIndex,
    ModuleInfo, SignatureIndex, Target,
};
use wasmer_types::{CustomSection, CustomSectionProtection, SectionIndex};
use wasmer_vm::{
    FunctionBodyPtr, SectionBodyPtr, SignatureRegistry, VMFunctionBody, VMSharedSignatureIndex,
    VMTrampoline,
};

/// A WebAssembly `Universal` Engine.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Mutex<EngineInner>>,
    /// The target for the compiler
    target: Arc<Target>,
    engine_id: EngineId,
}

impl Engine {
    /// Create a new `Engine` with the given config
    #[cfg(feature = "universal_engine")]
    pub fn new(compiler: Box<dyn Compiler>, target: Target, features: Features) -> Self {
        Self {
            inner: Arc::new(Mutex::new(EngineInner {
                builder: EngineBuilder::new(Some(compiler), features),
                code_memory: vec![],
                signatures: SignatureRegistry::new(),
            })),
            target: Arc::new(target),
            engine_id: EngineId::default(),
        }
    }

    /// Create a headless `Engine`
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
            inner: Arc::new(Mutex::new(EngineInner {
                builder: EngineBuilder::new(None, Features::default()),
                code_memory: vec![],
                signatures: SignatureRegistry::new(),
            })),
            target: Arc::new(Target::default()),
            engine_id: EngineId::default(),
        }
    }

    pub(crate) fn inner(&self) -> std::sync::MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap()
    }

    pub(crate) fn inner_mut(&self) -> std::sync::MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap()
    }

    /// Gets the target
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// Register a signature
    pub fn register_signature(&self, func_type: &FunctionType) -> VMSharedSignatureIndex {
        let compiler = self.inner();
        compiler.signatures().register(func_type)
    }

    /// Lookup a signature
    pub fn lookup_signature(&self, sig: VMSharedSignatureIndex) -> Option<FunctionType> {
        let compiler = self.inner();
        compiler.signatures().lookup(sig)
    }

    /// Validates a WebAssembly module
    pub fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        self.inner().validate(binary)
    }

    /// Compile a WebAssembly binary
    #[cfg(feature = "universal_engine")]
    pub fn compile(
        &self,
        binary: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Arc<Artifact>, CompileError> {
        Ok(Arc::new(Artifact::new(self, binary, tunables)?))
    }

    /// Compile a WebAssembly binary
    #[cfg(not(feature = "universal_engine"))]
    pub fn compile(
        &self,
        _binary: &[u8],
        _tunables: &dyn Tunables,
    ) -> Result<Arc<Artifact>, CompileError> {
        Err(CompileError::Codegen(
            "The Engine is operating in headless mode, so it can not compile Modules.".to_string(),
        ))
    }

    /// Deserializes a WebAssembly module
    ///
    /// # Safety
    ///
    /// The serialized content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize(&self, bytes: &[u8]) -> Result<Arc<Artifact>, DeserializeError> {
        Ok(Arc::new(Artifact::deserialize(self, bytes)?))
    }

    /// Deserializes a WebAssembly module from a path
    ///
    /// # Safety
    ///
    /// The file's content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        let file = std::fs::File::open(file_ref)?;
        let mmap = Mmap::map(&file)?;
        self.deserialize(&mmap)
    }

    /// A unique identifier for this object.
    ///
    /// This exists to allow us to compare two Engines for equality. Otherwise,
    /// comparing two trait objects unsafely relies on implementation details
    /// of trait representation.
    pub fn id(&self) -> &EngineId {
        &self.engine_id
    }

    /// Clone the engine
    pub fn cloned(&self) -> Arc<Self> {
        Arc::new(self.clone())
    }
}

/// The inner contents of `Engine`
pub struct EngineInner {
    /// The builder (include compiler and cpu features)
    builder: EngineBuilder,
    /// The code memory is responsible of publishing the compiled
    /// functions to memory.
    code_memory: Vec<CodeMemory>,
    /// The signature registry is used mainly to operate with trampolines
    /// performantly.
    signatures: SignatureRegistry,
}

impl EngineInner {
    /// Gets the compiler associated to this engine.
    #[cfg(feature = "universal_engine")]
    pub fn compiler(&self) -> Result<&dyn Compiler, CompileError> {
        self.builder.compiler()
    }

    /// Validate the module
    pub fn validate(&self, data: &[u8]) -> Result<(), CompileError> {
        self.builder.validate(data)
    }

    /// The Wasm features
    pub fn features(&self) -> &Features {
        self.builder.features()
    }

    pub fn builder_mut(&mut self) -> &mut EngineBuilder {
        &mut self.builder
    }

    /// Allocate compiled functions into memory
    #[allow(clippy::type_complexity)]
    pub(crate) fn allocate(
        &mut self,
        _module: &ModuleInfo,
        functions: &PrimaryMap<LocalFunctionIndex, FunctionBody>,
        function_call_trampolines: &PrimaryMap<SignatureIndex, FunctionBody>,
        dynamic_function_trampolines: &PrimaryMap<FunctionIndex, FunctionBody>,
        custom_sections: &PrimaryMap<SectionIndex, CustomSection>,
    ) -> Result<
        (
            PrimaryMap<LocalFunctionIndex, FunctionExtent>,
            PrimaryMap<SignatureIndex, VMTrampoline>,
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
            .map(|slice| FunctionExtent {
                ptr: FunctionBodyPtr(slice.as_ptr()),
                length: slice.len(),
            })
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let mut allocated_function_call_trampolines: PrimaryMap<SignatureIndex, VMTrampoline> =
            PrimaryMap::new();
        for ptr in allocated_functions
            .drain(0..function_call_trampolines.len())
            .map(|slice| slice.as_ptr())
        {
            let trampoline =
                unsafe { std::mem::transmute::<*const VMFunctionBody, VMTrampoline>(ptr) };
            allocated_function_call_trampolines.push(trampoline);
        }

        let allocated_dynamic_function_trampolines = allocated_functions
            .drain(..)
            .map(|slice| FunctionBodyPtr(slice.as_ptr()))
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

    /// Register DWARF-type exception handling information associated with the code.
    pub(crate) fn publish_eh_frame(&mut self, eh_frame: Option<&[u8]>) -> Result<(), CompileError> {
        self.code_memory
            .last_mut()
            .unwrap()
            .unwind_registry_mut()
            .publish(eh_frame)
            .map_err(|e| {
                CompileError::Resource(format!("Error while publishing the unwind code: {}", e))
            })?;
        Ok(())
    }

    /// Shared signature registry.
    pub fn signatures(&self) -> &SignatureRegistry {
        &self.signatures
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
/// A unique identifier for an Engine.
pub struct EngineId {
    id: usize,
}

impl EngineId {
    /// Format this identifier as a string.
    pub fn id(&self) -> String {
        format!("{}", &self.id)
    }
}

impl Clone for EngineId {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Default for EngineId {
    fn default() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, SeqCst),
        }
    }
}
