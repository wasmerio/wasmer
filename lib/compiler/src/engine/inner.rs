use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, RwLock};

use crate::engine::builder::EngineBuilder;
#[cfg(feature = "compiler")]
use crate::{Compiler, CompilerConfig, Debugger};

#[cfg(not(target_arch = "wasm32"))]
use wasmer_types::CompilationProgressCallback;
#[cfg(feature = "compiler")]
use wasmer_types::Features;
use wasmer_types::{CompileError, target::Target};

#[cfg(not(target_arch = "wasm32"))]
use shared_buffer::OwnedBuffer;
#[cfg(not(target_arch = "wasm32"))]
use std::ffi::c_void;
#[cfg(all(not(target_arch = "wasm32"), feature = "compiler"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Seek};
#[cfg(all(not(target_arch = "wasm32"), unix))]
use std::os::fd::RawFd;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(all(not(target_arch = "wasm32"), feature = "compiler"))]
use wasmer_types::ModuleInfo;
#[cfg(not(target_arch = "wasm32"))]
use wasmer_types::{
    DeserializeError, FunctionIndex, FunctionType, LocalFunctionIndex, SignatureHash,
    SignatureIndex, entity::PrimaryMap,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::{
    Artifact, BaseTunables, CodeMemory, FunctionExtent, GlobalFrameInfoRegistration, Tunables,
    engine::mapped_binary::MemoryMappedBinary,
    types::{
        function::FunctionBodyLike,
        section::{CustomSectionLike, CustomSectionProtection, SectionIndex},
    },
};

#[cfg(not(target_arch = "wasm32"))]
use wasmer_vm::{
    FunctionBodyPtr, SectionBodyPtr, SignatureRegistry, VMFunctionBody, VMSignatureHash,
    VMTrampoline,
};

#[derive(Debug)]
struct EngineMetadata {
    deterministic_id: String,
    artifact_format: String,
}

impl EngineMetadata {
    #[cfg(feature = "compiler")]
    fn from_compiler(compiler: Option<&dyn Compiler>, fallback: &str) -> Self {
        match compiler {
            Some(compiler) => Self {
                deterministic_id: compiler.deterministic_id(),
                artifact_format: compiler.artifact_format(),
            },
            None => Self {
                deterministic_id: fallback.to_string(),
                artifact_format: fallback.to_string(),
            },
        }
    }

    #[cfg(not(feature = "compiler"))]
    fn headless(fallback: &str) -> Self {
        Self {
            deterministic_id: fallback.to_string(),
            artifact_format: fallback.to_string(),
        }
    }
}

/// A WebAssembly Engine.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Mutex<EngineInner>>,
    metadata: Arc<RwLock<EngineMetadata>>,
    /// The target for the compiler
    target: Arc<Target>,
    engine_id: EngineId,
    #[cfg(not(target_arch = "wasm32"))]
    tunables: Arc<dyn Tunables + Send + Sync>,
    name: String,
}

impl Engine {
    /// Create a new `Engine` with the given config
    #[cfg(feature = "compiler")]
    pub fn new(
        compiler_config: Box<dyn CompilerConfig>,
        target: Target,
        features: Features,
    ) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let tunables = BaseTunables::new();
        let compiler = compiler_config.compiler();
        let name = format!("engine-{}", compiler.name());
        let metadata = EngineMetadata::from_compiler(Some(compiler.as_ref()), &name);
        Self {
            inner: Arc::new(Mutex::new(EngineInner {
                compiler: Some(compiler),
                features,
                #[cfg(not(target_arch = "wasm32"))]
                code_memory: vec![],
                #[cfg(not(target_arch = "wasm32"))]
                elf_mapped_binary: vec![],
                #[cfg(not(target_arch = "wasm32"))]
                signatures: SignatureRegistry::new(),
            })),
            metadata: Arc::new(RwLock::new(metadata)),
            target: Arc::new(target),
            engine_id: EngineId::default(),
            #[cfg(not(target_arch = "wasm32"))]
            tunables: Arc::new(tunables),
            name,
        }
    }

    /// Returns the name of this engine
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the deterministic id of this engine
    pub fn deterministic_id(&self) -> String {
        self.metadata.read().unwrap().deterministic_id.clone()
    }

    /// Returns the format used for artifacts produced by this engine.
    pub fn artifact_format(&self) -> String {
        self.metadata.read().unwrap().artifact_format.clone()
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
        let target = Target::default();
        let name = "engine-headless".to_string();
        #[cfg(not(target_arch = "wasm32"))]
        let tunables = BaseTunables::new();
        Self {
            inner: Arc::new(Mutex::new(EngineInner {
                #[cfg(feature = "compiler")]
                compiler: None,
                #[cfg(feature = "compiler")]
                features: Features::default(),
                #[cfg(not(target_arch = "wasm32"))]
                code_memory: vec![],
                #[cfg(not(target_arch = "wasm32"))]
                elf_mapped_binary: vec![],
                #[cfg(not(target_arch = "wasm32"))]
                signatures: SignatureRegistry::new(),
            })),
            metadata: Arc::new(RwLock::new({
                #[cfg(feature = "compiler")]
                {
                    EngineMetadata::from_compiler(None, &name)
                }
                #[cfg(not(feature = "compiler"))]
                {
                    EngineMetadata::headless(&name)
                }
            })),
            target: Arc::new(target),
            engine_id: EngineId::default(),
            #[cfg(not(target_arch = "wasm32"))]
            tunables: Arc::new(tunables),
            name,
        }
    }

    /// Get reference to `EngineInner`.
    pub fn inner(&self) -> std::sync::MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap()
    }

    /// Get mutable reference to `EngineInner`.
    pub fn inner_mut(&self) -> std::sync::MutexGuard<'_, EngineInner> {
        self.inner.lock().unwrap()
    }

    /// Gets the target
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// Register a signature
    #[cfg(not(target_arch = "wasm32"))]
    pub fn register_signature(&self, func_type: &FunctionType) -> VMSignatureHash {
        let compiler = self.inner();
        compiler
            .signatures()
            .register(func_type, SignatureHash(func_type.signature_hash()))
    }

    /// Look up a registered signature by its hash.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn lookup_signature(&self, sig_hash: VMSignatureHash) -> Option<FunctionType> {
        let compiler = self.inner();
        compiler.signatures().lookup_signature(sig_hash)
    }

    /// Validates a WebAssembly module
    #[cfg(feature = "compiler")]
    pub fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        self.inner().validate(binary)
    }

    /// Compile a WebAssembly binary
    #[cfg(feature = "compiler")]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compile(&self, binary: &[u8]) -> Result<Arc<Artifact>, CompileError> {
        Ok(Arc::new(Artifact::new(
            self,
            binary,
            self.tunables.as_ref(),
            None,
        )?))
    }

    /// Compile a WebAssembly binary with a progress callback.
    #[cfg(feature = "compiler")]
    pub fn compile_with_progress(
        &self,
        binary: &[u8],
        progress_callback: Option<CompilationProgressCallback>,
    ) -> Result<Arc<Artifact>, CompileError> {
        Ok(Arc::new(Artifact::new(
            self,
            binary,
            self.tunables.as_ref(),
            progress_callback,
        )?))
    }

    /// Compile a WebAssembly binary (the progress_callback argument is unused).
    #[cfg(not(feature = "compiler"))]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compile_with_progress(
        &self,
        binary: &[u8],
        _progress_callback: Option<CompilationProgressCallback>,
    ) -> Result<Arc<Artifact>, CompileError> {
        self.compile(binary, self.tunables.as_ref())
    }

    /// Compile a WebAssembly binary
    #[cfg(not(feature = "compiler"))]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn compile(
        &self,
        _binary: &[u8],
        _tunables: &dyn Tunables,
    ) -> Result<Arc<Artifact>, CompileError> {
        Err(CompileError::Codegen(
            "The Engine is operating in headless mode, so it can not compile Modules.".to_string(),
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Deserializes a WebAssembly module which was previously serialized with
    /// [`wasmer::Module::serialize`].
    ///
    /// # Safety
    ///
    /// See [`Artifact::deserialize_unchecked`].
    pub unsafe fn deserialize_unchecked(
        &self,
        bytes: OwnedBuffer,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        unsafe { Ok(Arc::new(Artifact::deserialize_unchecked(self, bytes)?)) }
    }

    /// Deserializes a WebAssembly module which was previously serialized with
    /// [`wasmer::Module::serialize`].
    ///
    /// # Safety
    ///
    /// See [`Artifact::deserialize`].
    #[cfg(not(target_arch = "wasm32"))]
    pub unsafe fn deserialize(
        &self,
        bytes: OwnedBuffer,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        unsafe { Ok(Arc::new(Artifact::deserialize(self, bytes)?)) }
    }

    /// Deserializes a WebAssembly module from a path.
    ///
    /// # Safety
    /// See [`Artifact::deserialize`].
    #[cfg(not(target_arch = "wasm32"))]
    pub unsafe fn deserialize_from_file(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        unsafe {
            let mut file = std::fs::File::open(file_ref)?;
            let mut magic = [0; 4];
            let is_elf = file.read_exact(&mut magic).is_ok() && magic == object::elf::ELFMAG;
            if is_elf {
                return Ok(Arc::new(Artifact::deserialize_file(self, file_ref)?));
            }
            file.rewind()?;
            self.deserialize(
                OwnedBuffer::from_file(&file)
                    .map_err(|e| DeserializeError::Generic(e.to_string()))?,
            )
        }
    }

    /// Deserialize from a file path.
    ///
    /// # Safety
    ///
    /// See [`Artifact::deserialize_unchecked`].
    #[cfg(not(target_arch = "wasm32"))]
    pub unsafe fn deserialize_from_file_unchecked(
        &self,
        file_ref: &Path,
    ) -> Result<Arc<Artifact>, DeserializeError> {
        unsafe {
            let file = std::fs::File::open(file_ref)?;
            self.deserialize_unchecked(
                OwnedBuffer::from_file(&file)
                    .map_err(|e| DeserializeError::Generic(e.to_string()))?,
            )
        }
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
    pub fn cloned(&self) -> Self {
        self.clone()
    }

    /// Attach a Tunable to this engine
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_tunables(&mut self, tunables: impl Tunables + Send + Sync + 'static) {
        self.tunables = Arc::new(tunables);
    }

    /// Get a reference to attached Tunable of this engine
    #[cfg(not(target_arch = "wasm32"))]
    pub fn tunables(&self) -> &dyn Tunables {
        self.tunables.as_ref()
    }

    /// Add suggested optimizations to this engine.
    #[deprecated(note = "User compilation options are currently unused")]
    pub fn with_opts(
        &mut self,
        _suggested_opts: &wasmer_types::target::UserCompilerOptimizations,
    ) -> Result<(), CompileError> {
        Ok(())
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.deterministic_id())
    }
}

/// The inner contents of `Engine`
pub struct EngineInner {
    #[cfg(feature = "compiler")]
    /// The compiler and cpu features
    compiler: Option<Box<dyn Compiler>>,
    #[cfg(feature = "compiler")]
    /// The compiler and cpu features
    features: Features,
    /// The code memory is responsible of publishing the compiled
    /// functions to memory.
    #[cfg(not(target_arch = "wasm32"))]
    code_memory: Vec<CodeMemory>,
    /// Memory-mapped ELF artifact image, produced by `--experimental-artifact`.
    #[cfg(not(target_arch = "wasm32"))]
    elf_mapped_binary: Vec<MemoryMappedBinary>,
    /// The signature registry is used mainly to operate with trampolines
    /// performantly.
    #[cfg(not(target_arch = "wasm32"))]
    signatures: SignatureRegistry,
}

impl std::fmt::Debug for EngineInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut formatter = f.debug_struct("EngineInner");
        #[cfg(feature = "compiler")]
        {
            formatter.field("compiler", &self.compiler);
            formatter.field("features", &self.features);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            formatter.field("signatures", &self.signatures);
        }

        formatter.finish()
    }
}

impl EngineInner {
    /// Gets the compiler associated to this engine.
    #[cfg(feature = "compiler")]
    pub fn compiler(&self) -> Result<&dyn Compiler, CompileError> {
        match self.compiler.as_ref() {
            None => Err(CompileError::Codegen(
                "No compiler compiled into executable".to_string(),
            )),
            Some(compiler) => Ok(&**compiler),
        }
    }

    /// Validate the module
    #[cfg(feature = "compiler")]
    pub fn validate(&self, data: &[u8]) -> Result<(), CompileError> {
        let compiler = self.compiler()?;
        compiler.validate_module(&self.features, data)
    }

    /// The Wasm features
    #[cfg(feature = "compiler")]
    pub fn features(&self) -> &Features {
        &self.features
    }

    /// Allocate compiled functions into memory
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::type_complexity)]
    pub(crate) fn allocate<'a, FunctionBody, CustomSection>(
        &'a mut self,
        _module: &wasmer_types::ModuleInfo,
        functions: impl ExactSizeIterator<Item = &'a FunctionBody> + 'a,
        function_call_trampolines: impl ExactSizeIterator<Item = &'a FunctionBody> + 'a,
        dynamic_function_trampolines: impl ExactSizeIterator<Item = &'a FunctionBody> + 'a,
        custom_sections: impl ExactSizeIterator<Item = &'a CustomSection> + Clone + 'a,
    ) -> Result<
        (
            PrimaryMap<LocalFunctionIndex, FunctionExtent>,
            PrimaryMap<SignatureIndex, VMTrampoline>,
            PrimaryMap<FunctionIndex, FunctionBodyPtr>,
            PrimaryMap<SectionIndex, SectionBodyPtr>,
        ),
        CompileError,
    >
    where
        FunctionBody: FunctionBodyLike<'a> + 'a,
        CustomSection: CustomSectionLike<'a> + 'a,
    {
        let functions_len = functions.len();
        let function_call_trampolines_len = function_call_trampolines.len();

        let function_bodies = functions
            .chain(function_call_trampolines)
            .chain(dynamic_function_trampolines)
            .collect::<Vec<_>>();
        let (executable_sections, data_sections): (Vec<_>, _) = custom_sections
            .clone()
            .partition(|section| section.protection() == CustomSectionProtection::ReadExecute);
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
                        "failed to allocate memory for functions: {message}",
                    ))
                })?;

        let allocated_functions_result = allocated_functions
            .drain(0..functions_len)
            .map(|slice| FunctionExtent {
                ptr: FunctionBodyPtr(slice.as_ptr()),
                length: slice.len(),
            })
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let mut allocated_function_call_trampolines: PrimaryMap<SignatureIndex, VMTrampoline> =
            PrimaryMap::new();
        for ptr in allocated_functions
            .drain(0..function_call_trampolines_len)
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
            .map(|section| {
                SectionBodyPtr(
                    if section.protection() == CustomSectionProtection::ReadExecute {
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

    #[cfg(not(target_arch = "wasm32"))]
    /// Make memory containing compiled code executable.
    pub(crate) fn publish_compiled_code(&mut self) {
        self.code_memory.last_mut().unwrap().publish();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    /// Register DWARF-type exception handling information associated with the code.
    pub(crate) fn publish_eh_frame(&mut self, eh_frame: Option<&[u8]>) -> Result<(), CompileError> {
        self.code_memory
            .last_mut()
            .unwrap()
            .unwind_registry_mut()
            .publish_eh_frame(eh_frame)
            .map_err(|e| {
                CompileError::Resource(format!("Error while publishing the unwind code: {e}"))
            })?;
        Ok(())
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    /// Register macos-specific exception handling information associated with the code.
    pub(crate) fn publish_compact_unwind(
        &mut self,
        compact_unwind: &[u8],
        eh_personality_addr_in_got: Option<usize>,
    ) -> Result<(), CompileError> {
        self.code_memory
            .last_mut()
            .unwrap()
            .unwind_registry_mut()
            .publish_compact_unwind(compact_unwind, eh_personality_addr_in_got)
            .map_err(|e| {
                CompileError::Resource(format!("Error while publishing the unwind code: {e}"))
            })?;
        Ok(())
    }

    /// Memory-map a compiled ELF artifact image, keeping the mapping alive
    /// for the lifetime of the engine. Returns the base address of the
    /// mapping, which section/symbol offsets from the image are relative to.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn map_elf_binary<'a, R: object::ReadRef<'a>>(
        &mut self,
        object_file: &object::File<'a, R>,
        data: &[u8],
    ) -> Result<*mut c_void, CompileError> {
        let map = MemoryMappedBinary::try_from_bytes(object_file, data)
            .map_err(CompileError::Resource)?;
        let base = map.base();
        self.elf_mapped_binary.push(map);
        Ok(base)
    }

    /// Memory-map a compiled ELF artifact directly from a file.
    #[cfg(all(not(target_arch = "wasm32"), unix))]
    pub(crate) fn map_elf_binary_file<'a, R: object::ReadRef<'a>>(
        &mut self,
        object_file: &object::File<'a, R>,
        file: RawFd,
    ) -> Result<*mut c_void, CompileError> {
        let map =
            MemoryMappedBinary::try_from_file(object_file, file).map_err(CompileError::Resource)?;
        let base = map.base();
        self.elf_mapped_binary.push(map);
        Ok(base)
    }

    #[cfg(all(not(target_arch = "wasm32"), unix, feature = "compiler"))]
    pub(crate) fn debugger(&self) -> Option<Debugger> {
        self.compiler
            .as_ref()
            .and_then(|compiler| compiler.get_debugger())
    }

    #[cfg(all(not(target_arch = "wasm32"), unix, feature = "compiler"))]
    pub(crate) fn register_debugger(
        &self,
        path: &Path,
        base: *mut c_void,
        debugger: Debugger,
    ) -> Result<(), CompileError> {
        use std::io::Write as _;

        let path = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string();
        let (command, source_command) = match debugger {
            Debugger::Gdb => (
                format!("add-symbol-file \"{path}\" -o 0x{:x}", base as usize),
                "source",
            ),
            Debugger::Lldb => (
                format!(
                    "target modules add \"{path}\"\ntarget modules load --file \"{path}\" --slide 0x{:x}",
                    base as usize
                ),
                "command source",
            ),
        };
        let filename = format!(
            "/tmp/wasmer-{}.{}",
            debugger.to_string().to_lowercase(),
            std::process::id()
        );
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&filename)
            .map_err(|error| CompileError::Resource(format!("Cannot open {filename}: {error}")))?;
        writeln!(file, "{command}")
            .map_err(|error| CompileError::Resource(format!("Cannot write {filename}: {error}")))?;

        eprintln!("**************************");
        eprintln!("For debugging under {debugger}, use: {source_command} {filename}");
        eprintln!("**************************");
        Ok(())
    }

    /// Register DWARF-type exception handling information associated with the code.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn publish_elf_eh_frame(
        &mut self,
        address: u64,
        size: u64,
    ) -> Result<(), CompileError> {
        self.elf_mapped_binary
            .last_mut()
            .unwrap()
            .publish_eh_frame_section(address, size)
            .map_err(|e| {
                CompileError::Resource(format!("Error while publishing the unwind code: {e}"))
            })
    }

    /// Shared signature registry.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn signatures(&self) -> &SignatureRegistry {
        &self.signatures
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Register the frame info for the code memory
    pub(crate) fn register_frame_info(&mut self, frame_info: GlobalFrameInfoRegistration) {
        self.code_memory
            .last_mut()
            .unwrap()
            .register_frame_info(frame_info);
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Register the frame info for the most recently mapped ELF binary.
    pub(crate) fn register_elf_frame_info(&mut self, frame_info: GlobalFrameInfoRegistration) {
        self.elf_mapped_binary
            .last_mut()
            .unwrap()
            .register_frame_info(frame_info);
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "compiler"))]
    pub(crate) fn register_perfmap(
        &self,
        finished_functions: &PrimaryMap<LocalFunctionIndex, FunctionExtent>,
        module_info: &ModuleInfo,
    ) -> Result<(), CompileError> {
        if self
            .compiler
            .as_ref()
            .is_some_and(|v| v.get_perfmap_enabled())
        {
            use std::fs::OpenOptions;

            let filename = format!("/tmp/perf-{}.map", std::process::id());
            // We might be loading shared libraries and so we must append to the file.
            let file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(&filename)
                .map_err(|e| {
                    CompileError::Codegen(format!("failed to open perf map file {filename}: {e}"))
                })?;
            let mut file = std::io::BufWriter::new(file);

            for (func_index, code) in finished_functions.iter() {
                let func_index = module_info.func_index(func_index);
                if let Some(func_name) = module_info.function_names.get(&func_index) {
                    let sanitized_name = func_name.replace(['\n', '\r'], "_");
                    let line = format!(
                        "{:p} {:x} {sanitized_name}\n",
                        code.ptr.0 as *const _, code.length
                    );
                    write!(file, "{line}").map_err(|e| CompileError::Codegen(e.to_string()))?;
                }
            }

            file.flush()
                .map_err(|e| CompileError::Codegen(e.to_string()))?;
        }

        Ok(())
    }
}

#[cfg(feature = "compiler")]
impl From<Box<dyn CompilerConfig>> for Engine {
    fn from(config: Box<dyn CompilerConfig>) -> Self {
        EngineBuilder::new(config).engine()
    }
}

impl From<EngineBuilder> for Engine {
    fn from(engine_builder: EngineBuilder) -> Self {
        engine_builder.engine()
    }
}

impl From<&Self> for Engine {
    fn from(engine_ref: &Self) -> Self {
        engine_ref.cloned()
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
        format!("{}", self.id)
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

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::Engine;

    #[cfg(feature = "compiler")]
    #[derive(Debug)]
    struct MutatingErrorCompiler {
        revision: usize,
    }

    #[cfg(feature = "compiler")]
    impl crate::Compiler for MutatingErrorCompiler {
        fn name(&self) -> &str {
            "mutating-error"
        }

        fn deterministic_id(&self) -> String {
            format!("compiler-{}", self.revision)
        }

        fn artifact_format(&self) -> String {
            format!("format-{}", self.revision)
        }

        fn with_opts(
            &mut self,
            _suggested_compiler_opts: &wasmer_types::target::UserCompilerOptimizations,
        ) -> Result<(), wasmer_types::CompileError> {
            self.revision += 1;
            Err(wasmer_types::CompileError::Codegen(
                "options rejected after mutation".to_string(),
            ))
        }

        fn compile_module(
            &self,
            _target: &wasmer_types::target::Target,
            _module: &crate::types::module::CompileModuleInfo,
            _compile_info_blob: &[u8],
            _module_translation: &crate::ModuleTranslationState,
            _function_body_inputs: wasmer_types::entity::PrimaryMap<
                wasmer_types::LocalFunctionIndex,
                crate::FunctionBodyData<'_>,
            >,
            _progress_callback: Option<&wasmer_types::CompilationProgressCallback>,
        ) -> Result<crate::types::function::Compilation, wasmer_types::CompileError> {
            unreachable!("metadata tests do not compile modules")
        }

        fn get_middlewares(&self) -> &[std::sync::Arc<dyn crate::translator::ModuleMiddleware>] {
            &[]
        }
    }

    #[cfg(feature = "compiler")]
    struct MutatingErrorConfig;

    #[cfg(feature = "compiler")]
    impl crate::CompilerConfig for MutatingErrorConfig {
        fn compiler(self: Box<Self>) -> Box<dyn crate::Compiler> {
            Box::new(MutatingErrorCompiler { revision: 0 })
        }

        fn push_middleware(
            &mut self,
            _middleware: std::sync::Arc<dyn crate::translator::ModuleMiddleware>,
        ) {
        }
    }

    #[test]
    fn metadata_reads_do_not_wait_for_compilation_lock() {
        let engine = Engine::headless();
        let expected_id = engine.deterministic_id();
        let expected_format = engine.artifact_format();

        let compilation_engine = engine.clone();
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let compilation = std::thread::spawn(move || {
            // Artifact compilation holds this lock while translating and compiling Wasm.
            let _inner = compilation_engine.inner_mut();
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        locked_rx.recv().unwrap();

        let reader_engine = engine.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            result_tx
                .send((
                    reader_engine.deterministic_id(),
                    reader_engine.artifact_format(),
                    format!("{reader_engine:?}"),
                ))
                .unwrap();
        });

        let result = result_rx.recv_timeout(Duration::from_secs(1));
        release_tx.send(()).unwrap();
        compilation.join().unwrap();
        reader.join().unwrap();

        let (id, format, debug) =
            result.expect("engine metadata reads must not contend with compilation");
        assert_eq!(id, expected_id);
        assert_eq!(format, expected_format);
        assert_eq!(debug, expected_id);
    }

    #[cfg(feature = "compiler")]
    #[test]
    fn compiler_option_mutation_refreshes_metadata_for_all_clones_on_error() {
        let mut engine = Engine::new(
            Box::new(MutatingErrorConfig),
            wasmer_types::target::Target::default(),
            wasmer_types::Features::default(),
        );
        let clone = engine.clone();
        assert_eq!(clone.deterministic_id(), "compiler-0");
        assert_eq!(clone.artifact_format(), "format-0");

        engine
            .with_opts(&wasmer_types::target::UserCompilerOptimizations::default())
            .unwrap_err();

        assert_eq!(engine.deterministic_id(), "compiler-1");
        assert_eq!(clone.deterministic_id(), "compiler-1");
        assert_eq!(clone.artifact_format(), "format-1");
    }
}
