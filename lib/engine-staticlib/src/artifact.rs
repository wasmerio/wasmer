//! Define `StaticlibArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{StaticlibEngine, StaticlibEngineInner};
use crate::serialize::{ModuleMetadata, ModuleMetadataSymbolRegistry};
use enumset::EnumSet;
use loupe::MemoryUsage;
use std::collections::BTreeMap;
use std::error::Error;
use std::mem;
use std::sync::Arc;
use wasmer_compiler::{
    CompileError, CpuFeature, Features, OperatingSystem, SymbolRegistry, Triple,
};
#[cfg(feature = "compiler")]
use wasmer_compiler::{
    CompileModuleInfo, Compiler, FunctionBodyData, ModuleEnvironment, ModuleMiddlewareChain,
    ModuleTranslationState,
};
use wasmer_engine::{
    Artifact, DeserializeError, InstantiationError, MetadataHeader, SerializeError,
};
#[cfg(feature = "compiler")]
use wasmer_engine::{Engine, Tunables};
#[cfg(feature = "compiler")]
use wasmer_object::{emit_compilation, emit_data, get_object_for_target};
use wasmer_types::entity::EntityRef;
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
#[cfg(feature = "compiler")]
use wasmer_types::DataInitializer;
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
pub struct StaticlibArtifact {
    metadata: ModuleMetadata,
    module_bytes: Vec<u8>,
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
    #[loupe(skip)]
    finished_function_call_trampolines: BoxedSlice<SignatureIndex, VMTrampoline>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    func_data_registry: Arc<FuncDataRegistry>,
    /// Length of the serialized metadata
    metadata_length: usize,
    symbol_registry: ModuleMetadataSymbolRegistry,
    is_compiled: bool,
}

#[allow(dead_code)]
fn to_compile_error(err: impl Error) -> CompileError {
    CompileError::Codegen(format!("{}", err))
}

#[allow(dead_code)]
const WASMER_METADATA_SYMBOL: &[u8] = b"WASMER_METADATA";

impl StaticlibArtifact {
    // Mach-O header in Mac
    #[allow(dead_code)]
    const MAGIC_HEADER_MH_CIGAM_64: &'static [u8] = &[207, 250, 237, 254];

    // ELF Magic header for Linux (32 bit)
    #[allow(dead_code)]
    const MAGIC_HEADER_ELF_32: &'static [u8] = &[0x7f, b'E', b'L', b'F', 1];

    // ELF Magic header for Linux (64 bit)
    #[allow(dead_code)]
    const MAGIC_HEADER_ELF_64: &'static [u8] = &[0x7f, b'E', b'L', b'F', 2];

    // COFF Magic header for Windows (64 bit)
    #[allow(dead_code)]
    const MAGIC_HEADER_COFF_64: &'static [u8] = &[b'M', b'Z'];

    /// Check if the provided bytes look like `StaticlibArtifact`.
    ///
    /// This means, if the bytes look like a static object file in the
    /// target system.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        cfg_if::cfg_if! {
            if #[cfg(all(target_pointer_width = "64", target_vendor="apple"))] {
                bytes.starts_with(Self::MAGIC_HEADER_MH_CIGAM_64)
            }
            else if #[cfg(all(target_pointer_width = "64", target_os="linux"))] {
                bytes.starts_with(Self::MAGIC_HEADER_ELF_64)
            }
            else if #[cfg(all(target_pointer_width = "32", target_os="linux"))] {
                bytes.starts_with(Self::MAGIC_HEADER_ELF_32)
            }
            else if #[cfg(all(target_pointer_width = "64", target_os="windows"))] {
                bytes.starts_with(Self::MAGIC_HEADER_COFF_64)
            }
            else {
                false
            }
        }
    }

    #[cfg(feature = "compiler")]
    /// Generate a compilation
    fn generate_metadata<'data>(
        data: &'data [u8],
        features: &Features,
        compiler: &dyn Compiler,
        tunables: &dyn Tunables,
    ) -> Result<
        (
            CompileModuleInfo,
            PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
            Vec<DataInitializer<'data>>,
            Option<ModuleTranslationState>,
        ),
        CompileError,
    > {
        let environ = ModuleEnvironment::new();
        let translation = environ.translate(data).map_err(CompileError::Wasm)?;

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
        Ok((
            compile_info,
            translation.function_body_inputs,
            translation.data_initializers,
            translation.module_translation_state,
        ))
    }

    /// Compile a data buffer into a `StaticlibArtifact`, which can be statically linked against
    /// and run later.
    #[cfg(feature = "compiler")]
    pub fn new(
        engine: &StaticlibEngine,
        data: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Self, CompileError> {
        let mut engine_inner = engine.inner_mut();
        let target = engine.target();
        let compiler = engine_inner.compiler()?;
        let (compile_info, function_body_inputs, data_initializers, module_translation) =
            Self::generate_metadata(data, engine_inner.features(), compiler, tunables)?;

        let data_initializers = data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let target_triple = target.triple();

        // TODO: we currently supply all-zero function body lengths.
        // We don't know the lengths until they're compiled, yet we have to
        // supply the metadata as an input to the compile.
        let function_body_lengths = function_body_inputs
            .keys()
            .map(|_function_body| 0u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();

        let mut metadata = ModuleMetadata {
            compile_info,
            prefix: engine_inner.get_prefix(&data),
            data_initializers,
            function_body_lengths,
            cpu_features: target.cpu_features().as_u64(),
        };

        /*
        In the C file we need:
        - imports
        - exports

        to construct an api::Module which is a Store (can be passed in via argument) and an
        Arc<dyn Artifact> which means this struct which includes:
        - CompileModuleInfo
        - Features
        - ModuleInfo
        - MemoryIndex -> MemoryStyle
        - TableIndex -> TableStyle
        - LocalFunctionIndex -> FunctionBodyPtr // finished functions
        - FunctionIndex -> FunctionBodyPtr // finished dynamic function trampolines
        - SignatureIndex -> VMSharedSignatureindextureIndex // signatures
         */

        let serialized_data = bincode::serialize(&metadata).map_err(to_compile_error)?;
        let mut metadata_binary = vec![];
        metadata_binary.extend(MetadataHeader::new(serialized_data.len()));
        metadata_binary.extend(serialized_data);
        let metadata_length = metadata_binary.len();

        let (compile_info, symbol_registry) = metadata.split();

        let mut module = (*compile_info.module).clone();
        let middlewares = compiler.get_middlewares();
        middlewares.apply_on_module_info(&mut module);
        compile_info.module = Arc::new(module);

        let maybe_obj_bytes = compiler.experimental_native_compile_module(
            &target,
            &compile_info,
            module_translation.as_ref().unwrap(),
            &function_body_inputs,
            &symbol_registry,
            &metadata_binary,
        );

        let obj_bytes = if let Some(obj_bytes) = maybe_obj_bytes {
            obj_bytes?
        } else {
            let compilation = compiler.compile_module(
                &target,
                &metadata.compile_info,
                module_translation.as_ref().unwrap(),
                function_body_inputs,
            )?;
            // there's an ordering issue, but we can update function_body_lengths here.
            /*
            // We construct the function body lengths
            let function_body_lengths = compilation
            .get_function_bodies()
            .values()
            .map(|function_body| function_body.body.len() as u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();
             */
            let mut obj = get_object_for_target(&target_triple).map_err(to_compile_error)?;
            emit_data(&mut obj, WASMER_METADATA_SYMBOL, &metadata_binary, 1)
                .map_err(to_compile_error)?;
            emit_compilation(&mut obj, compilation, &symbol_registry, &target_triple)
                .map_err(to_compile_error)?;
            obj.write().map_err(to_compile_error)?
        };

        Self::from_parts_crosscompiled(&mut *engine_inner, metadata, obj_bytes, metadata_length)
    }

    /// Get the default extension when serializing this artifact
    pub fn get_default_extension(triple: &Triple) -> &'static str {
        match triple.operating_system {
            OperatingSystem::Windows => "obj",
            _ => "o",
        }
    }

    /// Construct a `StaticlibArtifact` from component parts.
    pub fn from_parts_crosscompiled(
        engine_inner: &mut StaticlibEngineInner,
        metadata: ModuleMetadata,
        module_bytes: Vec<u8>,
        metadata_length: usize,
    ) -> Result<Self, CompileError> {
        let finished_functions: PrimaryMap<LocalFunctionIndex, FunctionBodyPtr> = PrimaryMap::new();
        let finished_function_call_trampolines: PrimaryMap<SignatureIndex, VMTrampoline> =
            PrimaryMap::new();
        let finished_dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBodyPtr> =
            PrimaryMap::new();
        let signature_registry = engine_inner.signatures();
        let signatures = metadata
            .compile_info
            .module
            .signatures
            .values()
            .map(|sig| signature_registry.register(sig))
            .collect::<PrimaryMap<_, _>>();

        let symbol_registry = metadata.get_symbol_registry();
        Ok(Self {
            metadata,
            module_bytes,
            finished_functions: finished_functions.into_boxed_slice(),
            finished_function_call_trampolines: finished_function_call_trampolines
                .into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
            func_data_registry: engine_inner.func_data().clone(),
            metadata_length,
            symbol_registry,
            is_compiled: true,
        })
    }

    /// Compile a data buffer into a `StaticlibArtifact`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(_engine: &StaticlibEngine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Deserialize a `StaticlibArtifact` from bytes.
    ///
    /// # Safety
    ///
    /// The bytes must represent a serialized WebAssembly module.
    pub unsafe fn deserialize(
        engine: &StaticlibEngine,
        bytes: &[u8],
    ) -> Result<Self, DeserializeError> {
        let metadata_len = MetadataHeader::parse(bytes)?;

        let metadata: ModuleMetadata =
            bincode::deserialize(&bytes[MetadataHeader::LEN..][..metadata_len]).unwrap();

        const WORD_SIZE: usize = mem::size_of::<usize>();
        let mut byte_buffer = [0u8; WORD_SIZE];

        let mut cur_offset = MetadataHeader::LEN + metadata_len;
        byte_buffer[0..WORD_SIZE].clone_from_slice(&bytes[cur_offset..(cur_offset + WORD_SIZE)]);
        cur_offset += WORD_SIZE;

        let num_finished_functions = usize::from_ne_bytes(byte_buffer);
        let mut finished_functions = PrimaryMap::new();

        let engine_inner = engine.inner();
        let signature_registry = engine_inner.signatures();
        let func_data_registry = engine_inner.func_data().clone();
        let mut sig_map: BTreeMap<SignatureIndex, VMSharedSignatureIndex> = BTreeMap::new();

        let num_imported_functions = metadata.compile_info.module.num_imported_functions;
        // set up the imported functions first...
        for i in 0..num_imported_functions {
            let sig_idx = metadata.compile_info.module.functions[FunctionIndex::new(i)];
            let func_type = &metadata.compile_info.module.signatures[sig_idx];
            let vm_shared_idx = signature_registry.register(&func_type);
            sig_map.insert(sig_idx, vm_shared_idx);
        }
        // read finished functions in order now...
        for i in 0..num_finished_functions {
            let local_func_idx = LocalFunctionIndex::new(i);
            let func_idx = metadata.compile_info.module.func_index(local_func_idx);
            let sig_idx = metadata.compile_info.module.functions[func_idx];
            let func_type = &metadata.compile_info.module.signatures[sig_idx];
            let vm_shared_idx = signature_registry.register(&func_type);
            sig_map.insert(sig_idx, vm_shared_idx);

            byte_buffer[0..WORD_SIZE]
                .clone_from_slice(&bytes[cur_offset..(cur_offset + WORD_SIZE)]);
            let fp = FunctionBodyPtr(usize::from_ne_bytes(byte_buffer) as _);
            cur_offset += WORD_SIZE;

            // TODO: we can read back the length here if we serialize it. This will improve debug output.

            finished_functions.push(fp);
        }

        let mut signatures: PrimaryMap<_, VMSharedSignatureIndex> = PrimaryMap::new();
        for i in 0..(sig_map.len()) {
            if let Some(shared_idx) = sig_map.get(&SignatureIndex::new(i)) {
                signatures.push(*shared_idx);
            } else {
                panic!("Invalid data, missing sig idx; TODO: handle this error");
            }
        }

        // read trampolines in order
        let mut finished_function_call_trampolines = PrimaryMap::new();
        byte_buffer[0..WORD_SIZE].clone_from_slice(&bytes[cur_offset..(cur_offset + WORD_SIZE)]);
        cur_offset += WORD_SIZE;
        let num_function_trampolines = usize::from_ne_bytes(byte_buffer);
        for _ in 0..num_function_trampolines {
            byte_buffer[0..WORD_SIZE]
                .clone_from_slice(&bytes[cur_offset..(cur_offset + WORD_SIZE)]);
            cur_offset += WORD_SIZE;
            let trampoline_ptr_bytes = usize::from_ne_bytes(byte_buffer);
            let trampoline = mem::transmute::<usize, VMTrampoline>(trampoline_ptr_bytes);
            finished_function_call_trampolines.push(trampoline);
            // TODO: we can read back the length here if we serialize it. This will improve debug output.
        }

        // read dynamic function trampolines in order now...
        let mut finished_dynamic_function_trampolines = PrimaryMap::new();
        byte_buffer[0..WORD_SIZE].clone_from_slice(&bytes[cur_offset..(cur_offset + WORD_SIZE)]);
        cur_offset += WORD_SIZE;
        let num_dynamic_trampoline_functions = usize::from_ne_bytes(byte_buffer);
        for _i in 0..num_dynamic_trampoline_functions {
            byte_buffer[0..WORD_SIZE]
                .clone_from_slice(&bytes[cur_offset..(cur_offset + WORD_SIZE)]);
            let fp = FunctionBodyPtr(usize::from_ne_bytes(byte_buffer) as _);
            cur_offset += WORD_SIZE;

            // TODO: we can read back the length here if we serialize it. This will improve debug output.

            finished_dynamic_function_trampolines.push(fp);
        }

        let symbol_registry = metadata.get_symbol_registry();
        Ok(Self {
            metadata,
            module_bytes: bytes.to_owned(),
            finished_functions: finished_functions.into_boxed_slice(),
            finished_function_call_trampolines: finished_function_call_trampolines
                .into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
            func_data_registry,
            metadata_length: 0,
            symbol_registry,
            is_compiled: false,
        })
    }

    /// Get the `SymbolRegistry` used to generate the names used in the Artifact.
    pub fn symbol_registry(&self) -> &dyn SymbolRegistry {
        &self.symbol_registry
    }

    /// The length in bytes of the metadata in the serialized output.
    pub fn metadata_length(&self) -> usize {
        self.metadata_length
    }
}

impl Artifact for StaticlibArtifact {
    fn module(&self) -> Arc<ModuleInfo> {
        self.metadata.compile_info.module.clone()
    }

    fn module_ref(&self) -> &ModuleInfo {
        &self.metadata.compile_info.module
    }

    fn module_mut(&mut self) -> Option<&mut ModuleInfo> {
        Arc::get_mut(&mut self.metadata.compile_info.module)
    }

    fn register_frame_info(&self) {
        // Do nothing for now
    }

    fn features(&self) -> &Features {
        &self.metadata.compile_info.features
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        EnumSet::from_u64(self.metadata.cpu_features)
    }

    fn data_initializers(&self) -> &[OwnedDataInitializer] {
        &*self.metadata.data_initializers
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.metadata.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.metadata.compile_info.table_styles
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

    fn preinstantiate(&self) -> Result<(), InstantiationError> {
        if self.is_compiled {
            panic!(
                "a module built with the staticlib engine must be linked \
                into the current executable"
            );
        }
        Ok(())
    }

    /// Serialize a StaticlibArtifact
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(self.module_bytes.clone())
    }
}
