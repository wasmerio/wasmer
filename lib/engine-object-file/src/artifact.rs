//! Define `ObjectFileArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{ObjectFileEngine, ObjectFileEngineInner};
use crate::serialize::ModuleMetadata;
use std::collections::BTreeMap;
use std::error::Error;
use std::mem;
use std::sync::Arc;
use wasmer_compiler::{CompileError, Features, OperatingSystem, Symbol, SymbolRegistry, Triple};
#[cfg(feature = "compiler")]
use wasmer_compiler::{
    CompileModuleInfo, FunctionBodyData, ModuleEnvironment, ModuleTranslationState,
};
use wasmer_engine::{Artifact, DeserializeError, InstantiationError, SerializeError};
#[cfg(feature = "compiler")]
use wasmer_engine::{Engine, Tunables};
#[cfg(feature = "compiler")]
use wasmer_object::{emit_compilation, emit_data, get_object_for_target};
use wasmer_types::entity::EntityRef;
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
#[cfg(feature = "compiler")]
use wasmer_types::DataInitializer;
use wasmer_types::{
    FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
use wasmer_vm::{
    FunctionBodyPtr, MemoryStyle, ModuleInfo, TableStyle, VMSharedSignatureIndex, VMTrampoline,
};

/// A compiled wasm module, ready to be instantiated.
pub struct ObjectFileArtifact {
    metadata: ModuleMetadata,
    module_bytes: Vec<u8>,
    finished_functions: BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, FunctionBodyPtr>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    /// Length of the serialized metadata
    metadata_length: usize,
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
        let metadata_length = metadata_binary.len();

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

        Self::from_parts_crosscompiled(&mut *engine_inner, metadata, obj_bytes, metadata_length)
    }

    /// Generate the header file that goes with the generated object file.
    pub fn generate_header_file(&self) -> String {
        let mut out = String::new();
        use std::fmt::Write;
        // TODO: double check this length (it's probably off by 10 or so)
        write!(
            &mut out,
            "const int module_bytes_len = {};\n",
            self.metadata_length
        )
        .unwrap();
        write!(&mut out, "extern const char WASMER_METADATA[];\n\n").unwrap();
        for (function_local_index, _function_len) in self.metadata.function_body_lengths.iter() {
            let function_name = self
                .metadata
                .symbol_to_name(Symbol::LocalFunction(function_local_index));
            // TODO: figure out the signtaure here too
            write!(&mut out, "void {}(void);\n", function_name).unwrap();
        }

        // function pointer array
        {
            write!(&mut out, "const void* function_pointers[] = {{\n").unwrap();
            for (function_local_index, _function_len) in self.metadata.function_body_lengths.iter()
            {
                let function_name = self
                    .metadata
                    .symbol_to_name(Symbol::LocalFunction(function_local_index));
                // TODO: figure out the signtaure here too
                write!(&mut out, "\t{},\n", function_name).unwrap();
                //write!(&mut out, "\t{},\n", function_len).unwrap();
            }
            write!(&mut out, "}};\n").unwrap();
        }

        write!(&mut out, "\n").unwrap();

        for (sig_index, _func_type) in self.metadata.compile_info.module.signatures.iter() {
            let function_name = self
                .metadata
                .symbol_to_name(Symbol::FunctionCallTrampoline(sig_index));

            write!(&mut out, "void {}(void*, void*, void*);\n", function_name).unwrap();
        }

        write!(&mut out, "\n").unwrap();

        // function trampolines
        {
            write!(&mut out, "const void* function_trampolines[] = {{\n").unwrap();
            for (sig_index, _vm_shared_index) in self.metadata.compile_info.module.signatures.iter()
            {
                let function_name = self
                    .metadata
                    .symbol_to_name(Symbol::FunctionCallTrampoline(sig_index));
                write!(&mut out, "\t{},\n", function_name).unwrap();
            }
            write!(&mut out, "}};\n").unwrap();
        }

        write!(&mut out, "\n").unwrap();

        for func_index in self
            .metadata
            .compile_info
            .module
            .functions
            .keys()
            .take(self.metadata.compile_info.module.num_imported_functions)
        {
            let function_name = self
                .metadata
                .symbol_to_name(Symbol::DynamicFunctionTrampoline(func_index));
            // TODO: figure out the signature here
            write!(&mut out, "void {}(void*, void*, void*);\n", function_name).unwrap();
        }

        // dynamic function trampoline pointer array
        {
            write!(
                &mut out,
                "const void* dynamic_function_trampoline_pointers[] = {{\n"
            )
            .unwrap();
            for func_index in self
                .metadata
                .compile_info
                .module
                .functions
                .keys()
                .take(self.metadata.compile_info.module.num_imported_functions)
            {
                let function_name = self
                    .metadata
                    .symbol_to_name(Symbol::DynamicFunctionTrampoline(func_index));
                // TODO: figure out the signature here
                write!(&mut out, "\t{},\n", function_name).unwrap();
            }
            write!(&mut out, "}};\n").unwrap();
        }

        out
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
        engine_inner: &mut ObjectFileEngineInner,
        metadata: ModuleMetadata,
        module_bytes: Vec<u8>,
        metadata_length: usize,
    ) -> Result<Self, CompileError> {
        let finished_functions: PrimaryMap<LocalFunctionIndex, FunctionBodyPtr> = PrimaryMap::new();
        let finished_dynamic_function_trampolines: PrimaryMap<FunctionIndex, FunctionBodyPtr> =
            PrimaryMap::new();
        //let signatures: PrimaryMap<SignatureIndex, VMSharedSignatureIndex> = PrimaryMap::new();
        let signature_registry = engine_inner.signatures();
        let signatures = metadata
            .compile_info
            .module
            .signatures
            .values()
            .map(|sig| signature_registry.register(sig))
            .collect::<PrimaryMap<_, _>>();

        Ok(Self {
            metadata,
            module_bytes,
            finished_functions: finished_functions.into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
            metadata_length,
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

        const WORD_SIZE: usize = mem::size_of::<usize>();
        let mut byte_buffer = [0u8; WORD_SIZE];

        let mut cur_offset = data_len + 10;
        for i in 0..WORD_SIZE {
            byte_buffer[i] = bytes[cur_offset + i];
        }
        cur_offset += WORD_SIZE;

        let num_finished_functions = usize::from_ne_bytes(byte_buffer);
        let mut finished_functions = PrimaryMap::new();

        #[repr(C)]
        struct SlicePtr {
            ptr: usize,
            len: usize,
        }

        let mut engine_inner = engine.inner_mut();
        let signature_registry = engine_inner.signatures();
        let mut sig_map: BTreeMap<SignatureIndex, VMSharedSignatureIndex> = BTreeMap::new();

        // read finished functions in order now...
        for i in 0..num_finished_functions {
            let sig_idx = metadata.compile_info.module.functions[FunctionIndex::new(i)];
            let func_type = &metadata.compile_info.module.signatures[sig_idx];
            let vm_shared_idx = signature_registry.register(&func_type);
            sig_map.insert(sig_idx, vm_shared_idx);

            let mut sp = SlicePtr { ptr: 0, len: 0 };
            for j in 0..WORD_SIZE {
                byte_buffer[j] = bytes[cur_offset + j];
            }
            sp.ptr = usize::from_ne_bytes(byte_buffer);
            cur_offset += WORD_SIZE;
            // REVIEW: we can also serialize and read back lengths, do we want to do this?
            /*for j in 0..WORD_SIZE {
                byte_buffer[j] = bytes[cur_offset + j];
            }
                sp.len = usize::from_ne_bytes(byte_buffer);
                cur_offset += WORD_SIZE;*/

            let fp = FunctionBodyPtr(mem::transmute(sp));
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
        for i in 0..WORD_SIZE {
            byte_buffer[i] = bytes[cur_offset + i];
        }
        cur_offset += WORD_SIZE;
        let num_function_trampolines = usize::from_ne_bytes(byte_buffer);
        for i in 0..num_function_trampolines {
            for j in 0..WORD_SIZE {
                byte_buffer[j] = bytes[cur_offset + j];
            }
            cur_offset += WORD_SIZE;
            let trampoline_ptr_bytes = usize::from_ne_bytes(byte_buffer);
            let trampoline = mem::transmute::<usize, VMTrampoline>(trampoline_ptr_bytes);

            let func_type = &metadata.compile_info.module.signatures[SignatureIndex::new(i)];

            engine_inner.add_trampoline(func_type, trampoline);
            // REVIEW: we can also serialize and read back lengths, do we want to do this?
            /*for j in 0..WORD_SIZE {
                byte_buffer[j] = bytes[cur_offset + j];
            }
                sp.len = usize::from_ne_bytes(byte_buffer);
                cur_offset += WORD_SIZE;*/
        }

        // read dynamic function trampolines in order now...
        let mut finished_dynamic_function_trampolines = PrimaryMap::new();
        for i in 0..WORD_SIZE {
            byte_buffer[i] = bytes[cur_offset + i];
        }
        cur_offset += WORD_SIZE;
        let num_dynamic_trampoline_functions = usize::from_ne_bytes(byte_buffer);
        for _i in 0..num_dynamic_trampoline_functions {
            let mut sp = SlicePtr { ptr: 0, len: 0 };
            for j in 0..WORD_SIZE {
                byte_buffer[j] = bytes[cur_offset + j];
            }
            sp.ptr = usize::from_ne_bytes(byte_buffer);
            cur_offset += WORD_SIZE;

            // REVIEW: we can also serialize and read back lengths, do we want to do this?
            /*for j in 0..WORD_SIZE {
                byte_buffer[j] = bytes[cur_offset + j];
            }
                sp.len = usize::from_ne_bytes(byte_buffer);
                cur_offset += WORD_SIZE;*/

            let fp = FunctionBodyPtr(mem::transmute(sp));

            finished_dynamic_function_trampolines.push(fp);
        }

        Ok(Self {
            metadata,
            // TODO: review
            module_bytes: vec![],
            finished_functions: finished_functions.into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
            metadata_length: 0,
        })
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

    fn create_header_file(&self) -> Option<String> {
        Some(self.generate_header_file())
    }

    fn preinstantiate(&self) -> Result<(), InstantiationError> {
        //todo!("figure out what preinstantiate means here");
        Ok(())
    }

    /// Serialize a ObjectFileArtifact
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(self.module_bytes.clone())
    }
}
