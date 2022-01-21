//! Define `UniversalArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{UniversalEngine, UniversalEngineInner};
use crate::link::link_module;
#[cfg(feature = "compiler")]
use crate::serialize::SerializableCompilation;
use crate::serialize::SerializableModule;
use crate::trampoline::{libcall_trampoline_len, make_libcall_trampolines};
use enumset::EnumSet;
use loupe::MemoryUsage;
use std::mem;
use std::sync::{Arc, Mutex};
use wasmer_compiler::{CompileError, CpuFeature, Features, Triple};
#[cfg(feature = "compiler")]
use wasmer_compiler::{CompileModuleInfo, ModuleEnvironment, ModuleMiddlewareChain};
use wasmer_engine::{
    register_frame_info, Artifact, DeserializeError, FunctionExtent, GlobalFrameInfoRegistration,
    MetadataHeader, SerializeError,
};
#[cfg(feature = "compiler")]
use wasmer_engine::{Engine, Tunables};
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
use wasmer_types::{
    FunctionIndex, LocalFunctionIndex, MemoryIndex, ModuleInfo, OwnedDataInitializer,
    SignatureIndex, TableIndex,
};
use wasmer_vm::{
    FuncDataRegistry, FunctionBodyPtr, MemoryStyle, TableStyle, VMSharedSignatureIndex,
    VMTrampoline,
};

/// A compiled wasm module, ready to be instantiated.
#[derive(MemoryUsage)]
pub struct UniversalArtifact {
    serializable: SerializableModule,
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
    #[loupe(skip)]
    finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    func_data_registry: Arc<FuncDataRegistry>,
    frame_info_registration: Mutex<Option<GlobalFrameInfoRegistration>>,
    finished_function_lengths: BoxedSlice<LocalFunctionIndex, usize>,
}

impl UniversalArtifact {
    const MAGIC_HEADER: &'static [u8; 16] = b"wasmer-universal";

    /// Check if the provided bytes look like a serialized `UniversalArtifact`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(Self::MAGIC_HEADER)
    }

    /// Compile a data buffer into a `UniversalArtifact`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(
        engine: &UniversalEngine,
        data: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();
        let mut inner_engine = engine.inner_mut();
        let features = inner_engine.features();

        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

        let compiler = inner_engine.compiler()?;

        // We try to apply the middleware first
        let mut module = translation.module;
        let middlewares = compiler.get_middlewares();
        middlewares.apply_on_module_info(&mut module);

        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();

        let compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features: features.clone(),
            memory_styles,
            table_styles,
        };

        // Compile the Module
        let compilation = compiler.compile_module(
            &engine.target(),
            &compile_info,
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
        )?;
        let function_call_trampolines = compilation.get_function_call_trampolines();
        let dynamic_function_trampolines = compilation.get_dynamic_function_trampolines();

        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let frame_infos = compilation.get_frame_info();

        // Synthesize a custom section to hold the libcall trampolines.
        let mut custom_sections = compilation.get_custom_sections();
        let mut custom_section_relocations = compilation.get_custom_section_relocations();
        let libcall_trampolines_section = make_libcall_trampolines(engine.target());
        custom_section_relocations.push(libcall_trampolines_section.relocations.clone());
        let libcall_trampolines = custom_sections.push(libcall_trampolines_section);
        let libcall_trampoline_len = libcall_trampoline_len(engine.target()) as u32;

        let serializable_compilation = SerializableCompilation {
            function_bodies: compilation.get_function_bodies(),
            function_relocations: compilation.get_relocations(),
            function_jt_offsets: compilation.get_jt_offsets(),
            function_frame_info: frame_infos,
            function_call_trampolines,
            dynamic_function_trampolines,
            custom_sections,
            custom_section_relocations,
            debug: compilation.get_debug(),
            libcall_trampolines,
            libcall_trampoline_len,
        };
        let serializable = SerializableModule {
            compilation: serializable_compilation,
            compile_info,
            data_initializers,
            cpu_features: engine.target().cpu_features().as_u64(),
        };
        Self::from_parts(&mut inner_engine, serializable)
    }

    /// Compile a data buffer into a `UniversalArtifact`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(_engine: &UniversalEngine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Deserialize a UniversalArtifact
    ///
    /// # Safety
    /// This function is unsafe because rkyv reads directly without validating
    /// the data.
    pub unsafe fn deserialize(
        universal: &UniversalEngine,
        bytes: &[u8],
    ) -> Result<Self, DeserializeError> {
        if !Self::is_deserializable(bytes) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not wasmer-universal".to_string(),
            ));
        }
        let bytes = &bytes[Self::MAGIC_HEADER.len()..];
        let metadata_len = MetadataHeader::parse(bytes)?;
        let metadata_slice: &[u8] = &bytes[MetadataHeader::LEN..][..metadata_len];
        let serializable = SerializableModule::deserialize(metadata_slice)?;
        Self::from_parts(&mut universal.inner_mut(), serializable)
            .map_err(DeserializeError::Compiler)
    }

    /// Construct a `UniversalArtifact` from component parts.
    pub fn from_parts(
        inner_engine: &mut UniversalEngineInner,
        serializable: SerializableModule,
    ) -> Result<Self, CompileError> {
        let (
            finished_functions,
            finished_function_call_trampolines,
            finished_dynamic_function_trampolines,
            custom_sections,
        ) = inner_engine.allocate(
            &serializable.compile_info.module,
            &serializable.compilation.function_bodies,
            &serializable.compilation.function_call_trampolines,
            &serializable.compilation.dynamic_function_trampolines,
            &serializable.compilation.custom_sections,
        )?;

        link_module(
            &serializable.compile_info.module,
            &finished_functions,
            &serializable.compilation.function_jt_offsets,
            serializable.compilation.function_relocations.clone(),
            &custom_sections,
            &serializable.compilation.custom_section_relocations,
            serializable.compilation.libcall_trampolines,
            serializable.compilation.libcall_trampoline_len as usize,
        );

        // Compute indices into the shared signature table.
        let signatures = {
            let signature_registry = inner_engine.signatures();
            serializable
                .compile_info
                .module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        let eh_frame = match &serializable.compilation.debug {
            Some(debug) => {
                let eh_frame_section_size = serializable.compilation.custom_sections
                    [debug.eh_frame]
                    .bytes
                    .len();
                let eh_frame_section_pointer = custom_sections[debug.eh_frame];
                Some(unsafe {
                    std::slice::from_raw_parts(*eh_frame_section_pointer, eh_frame_section_size)
                })
            }
            None => None,
        };

        // Make all code compiled thus far executable.
        inner_engine.publish_compiled_code();

        inner_engine.publish_eh_frame(eh_frame)?;

        let finished_function_lengths = finished_functions
            .values()
            .map(|extent| extent.length)
            .collect::<PrimaryMap<LocalFunctionIndex, usize>>()
            .into_boxed_slice();
        let finished_functions = finished_functions
            .values()
            .map(|extent| extent.ptr)
            .collect::<PrimaryMap<LocalFunctionIndex, FunctionBodyPtr>>()
            .into_boxed_slice();
        let finished_function_call_trampolines =
            finished_function_call_trampolines.into_boxed_slice();
        let finished_dynamic_function_trampolines =
            finished_dynamic_function_trampolines.into_boxed_slice();
        let signatures = signatures.into_boxed_slice();
        let func_data_registry = inner_engine.func_data().clone();

        Ok(Self {
            serializable,
            finished_functions,
            finished_function_call_trampolines,
            finished_dynamic_function_trampolines,
            signatures,
            frame_info_registration: Mutex::new(None),
            finished_function_lengths,
            func_data_registry,
        })
    }

    /// Get the default extension when serializing this artifact
    pub fn get_default_extension(_triple: &Triple) -> &'static str {
        // `.wasmu` is the default extension for all the triples. It
        // stands for “Wasm Universal”.
        "wasmu"
    }
}

impl Artifact for UniversalArtifact {
    fn module(&self) -> Arc<ModuleInfo> {
        self.serializable.compile_info.module.clone()
    }

    fn module_ref(&self) -> &ModuleInfo {
        &self.serializable.compile_info.module
    }

    fn module_mut(&mut self) -> Option<&mut ModuleInfo> {
        Arc::get_mut(&mut self.serializable.compile_info.module)
    }

    fn register_frame_info(&self) {
        let mut info = self.frame_info_registration.lock().unwrap();

        if info.is_some() {
            return;
        }

        let finished_function_extents = self
            .finished_functions
            .values()
            .copied()
            .zip(self.finished_function_lengths.values().copied())
            .map(|(ptr, length)| FunctionExtent { ptr, length })
            .collect::<PrimaryMap<LocalFunctionIndex, _>>()
            .into_boxed_slice();

        let frame_infos = &self.serializable.compilation.function_frame_info;
        *info = register_frame_info(
            self.serializable.compile_info.module.clone(),
            &finished_function_extents,
            frame_infos.clone(),
        );
    }

    fn features(&self) -> &Features {
        &self.serializable.compile_info.features
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        EnumSet::from_u64(self.serializable.cpu_features)
    }

    fn data_initializers(&self) -> &[OwnedDataInitializer] {
        &*self.serializable.data_initializers
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.serializable.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.serializable.compile_info.table_styles
    }

    fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, FunctionBodyPtr> {
        &self.finished_functions
    }

    fn finished_function_call_trampolines(&self) -> &BoxedSlice<SignatureIndex, VMTrampoline> {
        &self.finished_function_call_trampolines
    }

    fn finished_dynamic_function_trampolines(&self) -> &BoxedSlice<FunctionIndex, FunctionBodyPtr> {
        &self.finished_dynamic_function_trampolines
    }

    fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self.signatures
    }

    fn func_data_registry(&self) -> &FuncDataRegistry {
        &self.func_data_registry
    }
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let serialized_data = self.serializable.serialize()?;
        assert!(mem::align_of::<SerializableModule>() <= MetadataHeader::ALIGN);

        let mut metadata_binary = vec![];
        metadata_binary.extend(Self::MAGIC_HEADER);
        metadata_binary.extend(MetadataHeader::new(serialized_data.len()));
        metadata_binary.extend(serialized_data);
        Ok(metadata_binary)
    }
}
