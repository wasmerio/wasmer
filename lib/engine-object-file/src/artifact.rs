//! Define `ObjectFileArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{ObjectFileEngine, ObjectFileEngineInner};
use crate::serialize::ModuleMetadata;
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(feature = "compiler")]
use std::process::Command;
use std::sync::Arc;
use tempfile::NamedTempFile;
#[cfg(feature = "compiler")]
use tracing::trace;
use wasmer_compiler::{CompileError, Features, OperatingSystem, Symbol, SymbolRegistry, Triple};
#[cfg(feature = "compiler")]
use wasmer_compiler::{
    CompileModuleInfo, FunctionBodyData, ModuleEnvironment, ModuleTranslationState,
};
use wasmer_engine::{
    Artifact, DeserializeError, InstantiationError, LinkError, RuntimeError, SerializeError,
};
#[cfg(feature = "compiler")]
use wasmer_engine::{Engine, Tunables};
#[cfg(feature = "compiler")]
use wasmer_object::{emit_compilation, emit_data, get_object_for_target};
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
#[cfg(feature = "compiler")]
use wasmer_types::DataInitializer;
use wasmer_types::{
    FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
use wasmer_vm::{
    FunctionBodyPtr, MemoryStyle, ModuleInfo, TableStyle, VMFunctionBody, VMSharedSignatureIndex,
    VMTrampoline,
};

/// A compiled wasm module, ready to be instantiated.
pub struct ObjectFileArtifact {
    metadata: ModuleMetadata,
    module_bytes: Vec<u8>,
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
}

fn to_compile_error(err: impl Error) -> CompileError {
    CompileError::Codegen(format!("{}", err))
}

const WASMER_METADATA_SYMBOL: &[u8] = b"WASMER_METADATA";

impl ObjectFileArtifact {
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

    /// Check if the provided bytes look like `ObjectFileArtifact`.
    ///
    /// This means, if the bytes look like a shared object file in the target
    /// system.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        cfg_if::cfg_if! {
            if #[cfg(all(target_pointer_width = "64", target_os="macos"))] {
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
        let memory_styles: PrimaryMap<MemoryIndex, MemoryStyle> = translation
            .module
            .memories
            .values()
            .map(|memory_type| tunables.memory_style(memory_type))
            .collect();
        let table_styles: PrimaryMap<TableIndex, TableStyle> = translation
            .module
            .tables
            .values()
            .map(|table_type| tunables.table_style(table_type))
            .collect();

        let compile_info = CompileModuleInfo {
            module: Arc::new(translation.module),
            features: features.clone(),
            memory_styles,
            table_styles,
        };
        Ok((
            compile_info,
            translation.function_body_inputs,
            translation.data_initializers,
            translation.module_translation,
        ))
    }

    /// Compile a data buffer into a `ObjectFileArtifact`, which can be statically linked against
    /// and run later.
    #[cfg(feature = "compiler")]
    pub fn new(
        engine: &ObjectFileEngine,
        data: &[u8],
        tunables: &dyn Tunables,
    ) -> Result<Self, CompileError> {
        let mut engine_inner = engine.inner_mut();
        let target = engine.target();
        let compiler = engine_inner.compiler()?;
        let (compile_info, function_body_inputs, data_initializers, module_translation) =
            Self::generate_metadata(data, engine_inner.features(), tunables)?;

        let data_initializers = data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let target_triple = target.triple();

        /*
        // We construct the function body lengths
        let function_body_lengths = compilation
            .get_function_bodies()
            .values()
            .map(|function_body| function_body.body.len() as u64)
            .map(|_function_body| 0u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();
         */

        // TODO: we currently supply all-zero function body lengths.
        // We don't know the lengths until they're compiled, yet we have to
        // supply the metadata as an input to the compile.
        let function_body_lengths = function_body_inputs
            .keys()
            .map(|_function_body| 0u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();

        let metadata = ModuleMetadata {
            compile_info,
            prefix: engine_inner.get_prefix(&data),
            data_initializers,
            function_body_lengths,
        };

        // let wasm_info = generate_data_structures_for_c(&metadata);
        // generate_c(wasm_info);
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
        let mut metadata_binary = vec![0; 10];
        let mut writable = &mut metadata_binary[..];
        leb128::write::unsigned(&mut writable, serialized_data.len() as u64)
            .expect("Should write number");
        metadata_binary.extend(serialized_data);

        let maybe_obj_bytes = compiler.experimental_object_file_compile_module(
            &target,
            &metadata.compile_info,
            module_translation.as_ref().unwrap(),
            &function_body_inputs,
            &metadata,
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
            let mut obj = get_object_for_target(&target_triple).map_err(to_compile_error)?;
            emit_data(&mut obj, WASMER_METADATA_SYMBOL, &metadata_binary)
                .map_err(to_compile_error)?;
            emit_compilation(&mut obj, compilation, &metadata, &target_triple)
                .map_err(to_compile_error)?;
            obj.write().map_err(to_compile_error)?
        };

        //let host_target = Triple::host();
        //let is_cross_compiling = target_triple != &host_target;

        Self::from_parts_crosscompiled(metadata, obj_bytes)
    }

    /// Get the default extension when serializing this artifact
    pub fn get_default_extension(triple: &Triple) -> &'static str {
        match triple.operating_system {
            OperatingSystem::Windows => "obj",
            _ => "o",
        }
    }

    /// Construct a `ObjectFileArtifact` from component parts.
    pub fn from_parts_crosscompiled(
        metadata: ModuleMetadata,
        module_bytes: Vec<u8>,
    ) -> Result<Self, CompileError> {
        let finished_functions: PrimaryMap<LocalFunctionIndex, FunctionBodyPtr> = PrimaryMap::new();
        let finished_dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBodyPtr> =
            PrimaryMap::new();
        let signatures: PrimaryMap<SignatureIndex, VMSharedSignatureIndex> = PrimaryMap::new();
        Ok(Self {
            metadata,
            module_bytes,
            finished_functions: finished_functions.into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
        })
    }

    /// Construct a `ObjectFileArtifact` from component parts.
    pub fn from_parts(
        engine_inner: &mut ObjectFileEngineInner,
        metadata: ModuleMetadata,
    ) -> Result<Self, CompileError> {
        let mut finished_functions: PrimaryMap<LocalFunctionIndex, FunctionBodyPtr> =
            PrimaryMap::new();
        for (function_local_index, function_len) in metadata.function_body_lengths.iter() {
            let function_name =
                metadata.symbol_to_name(Symbol::LocalFunction(function_local_index));
            /*
            unsafe {
                // We use a fake function signature `fn()` because we just
                // want to get the function address.
                let func: LibrarySymbol<unsafe extern "C" fn()> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                let raw = *func.into_raw();
                // The function pointer is a fat pointer, however this information
                // is only used when retrieving the trap information which is not yet
                // implemented in this engine.
                let func_pointer =
                    std::slice::from_raw_parts(raw as *const (), *function_len as usize);
                let func_pointer = func_pointer as *const [()] as *mut [VMFunctionBody];
                finished_functions.push(FunctionBodyPtr(func_pointer));
            }
            */
            todo!("objectfileartifact:from_parts");
        }

        // Retrieve function call trampolines (for all signatures in the module)
        for (sig_index, func_type) in metadata.compile_info.module.signatures.iter() {
            let function_name = metadata.symbol_to_name(Symbol::FunctionCallTrampoline(sig_index));
            unsafe {
                let trampoline = todo!("Get tramploine in ObjectFileArtifact::from_parts");
                //engine_inner.add_trampoline(&func_type, *trampoline);
            }
        }

        // Retrieve dynamic function trampolines (only for imported functions)
        let mut finished_dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBodyPtr> =
            PrimaryMap::with_capacity(metadata.compile_info.module.num_imported_functions);
        for func_index in metadata
            .compile_info
            .module
            .functions
            .keys()
            .take(metadata.compile_info.module.num_imported_functions)
        {
            let function_name =
                metadata.symbol_to_name(Symbol::DynamicFunctionTrampoline(func_index));
            unsafe {
                /*let trampoline: LibrarySymbol<unsafe extern "C" fn()> = lib
                .get(function_name.as_bytes())
                .map_err(to_compile_error)?;*/
                todo!("Get trampoline in from_parts");
                /*let raw = *trampoline.into_raw();
                let trampoline_pointer = std::slice::from_raw_parts(raw as *const (), 0);
                let trampoline_pointer =
                    trampoline_pointer as *const [()] as *mut [VMFunctionBody];
                finished_dynamic_function_trampolines.push(FunctionBodyPtr(trampoline_pointer));*/
            }
        }

        // Leaving frame infos from now, as they are not yet used
        // however they might be useful for the future.
        // let frame_infos = compilation
        //     .get_frame_info()
        //     .values()
        //     .map(|frame_info| SerializableFunctionFrameInfo::Processed(frame_info.clone()))
        //     .collect::<PrimaryMap<LocalFunctionIndex, _>>();
        // Self::from_parts(&mut engine_inner, lib, metadata, )
        // let frame_info_registration = register_frame_info(
        //     serializable.module.clone(),
        //     &finished_functions,
        //     serializable.compilation.function_frame_info.clone(),
        // );

        // Compute indices into the shared signature table.
        let signatures = {
            let signature_registry = engine_inner.signatures();
            metadata
                .compile_info
                .module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        Ok(Self {
            metadata,
            // TOOD:
            module_bytes: vec![],
            finished_functions: finished_functions.into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
        })
    }

    /// Compile a data buffer into a `ObjectFileArtifact`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(_engine: &ObjectFileEngine, _data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Deserialize a `ObjectFileArtifact` from bytes.
    ///
    /// # Safety
    ///
    /// The bytes must represent a serialized WebAssembly module.
    pub unsafe fn deserialize(
        engine: &ObjectFileEngine,
        bytes: &[u8],
    ) -> Result<Self, DeserializeError> {
        let mut reader = bytes;
        let data_len = leb128::read::unsigned(&mut reader).unwrap() as usize;

        let metadata: ModuleMetadata = bincode::deserialize(&bytes[10..(data_len + 10)]).unwrap();

        // read finished functions in order now...
        // read dynamic function trampolines in order now...
        // read signatures in order now...

        Ok(Self {
            metadata,
            // TODO: review
            module_bytes: vec![],
            finished_functions: PrimaryMap::new().into_boxed_slice(),
            finished_dynamic_function_trampolines: PrimaryMap::new().into_boxed_slice(),
            signatures: PrimaryMap::new().into_boxed_slice(),
        })

        /*if !Self::is_deserializable(&bytes) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not in any object file format Wasmer can understand"
                    .to_string(),
            ));
        }*/
    }

    /// Deserialize a `ObjectFileArtifact` from a file path.
    ///
    /// # Safety
    ///
    /// The file's content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize_from_file(
        engine: &ObjectFileEngine,
        path: &Path,
    ) -> Result<Self, DeserializeError> {
        let mut file = File::open(&path)?;
        let mut buffer = [0; 5];
        // read up to 5 bytes
        file.read_exact(&mut buffer)?;
        if !Self::is_deserializable(&buffer) {
            return Err(DeserializeError::Incompatible(
                "The provided bytes are not in any object file format Wasmer can understand"
                    .to_string(),
            ));
        }
        Self::deserialize_from_file_unchecked(&engine, &path)
    }

    /// Deserialize a `ObjectFileArtifact` from a file path (unchecked).
    ///
    /// # Safety
    ///
    /// The file's content must represent a serialized WebAssembly module.
    pub unsafe fn deserialize_from_file_unchecked(
        engine: &ObjectFileEngine,
        path: &Path,
    ) -> Result<Self, DeserializeError> {
        todo!("ObjectFileArtifact::deserialize_from_file_unchecked");
        /*Self::from_parts(&mut engine_inner, metadata, shared_path)
        .map_err(DeserializeError::Compiler)*/
    }
}

impl Artifact for ObjectFileArtifact {
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

    fn finished_dynamic_function_trampolines(&self) -> &BoxedSlice<FunctionIndex, FunctionBodyPtr> {
        &self.finished_dynamic_function_trampolines
    }

    fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self.signatures
    }

    fn preinstantiate(&self) -> Result<(), InstantiationError> {
        /*if self.library.is_none() {
            return Err(InstantiationError::Link(LinkError::Trap(
                RuntimeError::new("Cross compiled artifacts can't be instantiated."),
            )));
        }
        Ok(())*/
        todo!("figure out what preinstantiate means here");
    }

    /// Serialize a ObjectFileArtifact
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(self.module_bytes.clone())
    }
}
