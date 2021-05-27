//! Define `UniversalArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{UniversalEngine, UniversalEngineInner};
use crate::link::link_module;
#[cfg(feature = "compiler")]
use crate::serialize::SerializableCompilation;
use crate::serialize::SerializableModule;
use loupe::MemoryUsage;
use std::sync::{Arc, Mutex};
use wasmer_compiler::{CompileError, Features, Triple};
#[cfg(feature = "compiler")]
use wasmer_compiler::{CompileModuleInfo, ModuleEnvironment, ModuleMiddlewareChain};
use wasmer_engine::{
    register_frame_info, Artifact, DeserializeError, FunctionExtent, GlobalFrameInfoRegistration,
    SerializeError,
};
#[cfg(feature = "compiler")]
use wasmer_engine::{Engine, Tunables};
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
use wasmer_types::{
    FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
use wasmer_vm::{
    FuncDataRegistry, FunctionBodyPtr, MemoryStyle, ModuleInfo, TableStyle, VMSharedSignatureIndex,
    VMTrampoline,
};

const SERIALIZED_METADATA_LENGTH_OFFSET: usize = 22;
const SERIALIZED_METADATA_CONTENT_OFFSET: usize = 32;

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
    const MAGIC_HEADER: &'static [u8; 22] = b"\0wasmer-universal\0\0\0\0\0";

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

        let serializable_compilation = SerializableCompilation {
            function_bodies: compilation.get_function_bodies(),
            function_relocations: compilation.get_relocations(),
            function_jt_offsets: compilation.get_jt_offsets(),
            function_frame_info: frame_infos,
            function_call_trampolines,
            dynamic_function_trampolines,
            custom_sections: compilation.get_custom_sections(),
            custom_section_relocations: compilation.get_custom_section_relocations(),
            debug: compilation.get_debug(),
        };
        let serializable = SerializableModule {
            compilation: serializable_compilation,
            compile_info,
            data_initializers,
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

        let mut inner_bytes = &bytes[SERIALIZED_METADATA_LENGTH_OFFSET..];

        let metadata_len = leb128::read::unsigned(&mut inner_bytes).map_err(|_e| {
            DeserializeError::CorruptedBinary("Can't read metadata size".to_string())
        })?;
        let metadata_slice: &[u8] = std::slice::from_raw_parts(
            &bytes[SERIALIZED_METADATA_CONTENT_OFFSET] as *const u8,
            metadata_len as usize,
        );

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
        // Prepend the header.
        let mut serialized = Self::MAGIC_HEADER.to_vec();

        serialized.resize(SERIALIZED_METADATA_CONTENT_OFFSET, 0);
        let mut writable_leb = &mut serialized[SERIALIZED_METADATA_LENGTH_OFFSET..];
        let serialized_data = self.serializable.serialize()?;
        let length = serialized_data.len();
        leb128::write::unsigned(&mut writable_leb, length as u64).expect("Should write number");

        let offset = pad_and_extend::<SerializableModule>(&mut serialized, &serialized_data);
        assert_eq!(offset, SERIALIZED_METADATA_CONTENT_OFFSET);

        Ok(serialized)
    }
}

/// It pads the data with the desired alignment
pub fn pad_and_extend<T>(prev_data: &mut Vec<u8>, data: &[u8]) -> usize {
    let align = std::mem::align_of::<T>();

    let mut offset = prev_data.len();
    if offset & (align - 1) != 0 {
        offset += align - (offset & (align - 1));
        prev_data.resize(offset, 0);
    }
    prev_data.extend(data);
    offset
}

#[cfg(test)]
mod tests {
    use super::pad_and_extend;

    #[test]
    fn test_pad_and_extend() {
        let mut data: Vec<u8> = vec![];
        let offset = pad_and_extend::<i64>(&mut data, &[1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(offset, 0);
        let offset = pad_and_extend::<i32>(&mut data, &[2, 0, 0, 0]);
        assert_eq!(offset, 8);
        let offset = pad_and_extend::<i64>(&mut data, &[3, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(offset, 16);
        assert_eq!(
            data,
            &[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0]
        );
    }
}
