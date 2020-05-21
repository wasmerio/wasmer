//! Define `NativeArtifact` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{NativeEngine, NativeEngineInner};
use crate::serialize::ModuleMetadata;
use faerie::{ArtifactBuilder, Decl, Link};
use libloading::{Library, Symbol};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tempfile::NamedTempFile;
use wasm_common::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasm_common::{
    FunctionIndex, LocalFunctionIndex, MemoryIndex, OwnedDataInitializer, SignatureIndex,
    TableIndex,
};
#[cfg(feature = "compiler")]
use wasmer_compiler::ModuleEnvironment;
use wasmer_compiler::{CompileError, Features, RelocationTarget};
use wasmer_engine::{Artifact, DeserializeError, Engine, SerializeError};
use wasmer_runtime::{MemoryPlan, TablePlan};
use wasmer_runtime::{ModuleInfo, VMFunctionBody, VMSharedSignatureIndex, VMTrampoline};

/// A compiled wasm module, ready to be instantiated.
pub struct NativeArtifact {
    sharedobject_path: PathBuf,
    metadata: ModuleMetadata,
    #[allow(dead_code)]
    library: Library,
    finished_functions: BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]>,
    finished_dynamic_function_trampolines: BoxedSlice<FunctionIndex, *const VMFunctionBody>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
}

fn to_compile_error(err: impl Error) -> CompileError {
    CompileError::Codegen(format!("{}", err))
}

impl NativeArtifact {
    /// Compile a data buffer into a `NativeArtifact`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(engine: &NativeEngine, data: &[u8]) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();
        let mut engine_inner = engine.inner_mut();
        let tunables = engine.tunables();

        let translation = environ
            .translate(data)
            .map_err(|error| CompileError::Wasm(error))?;

        let memory_plans: PrimaryMap<MemoryIndex, MemoryPlan> = translation
            .module
            .memories
            .iter()
            .map(|(_index, memory_type)| tunables.memory_plan(*memory_type))
            .collect();
        let table_plans: PrimaryMap<TableIndex, TablePlan> = translation
            .module
            .tables
            .iter()
            .map(|(_index, table_type)| tunables.table_plan(*table_type))
            .collect();

        let compiler = engine_inner.compiler()?;

        // Compile the Module
        let compilation = compiler.compile_module(
            &translation.module,
            translation.module_translation.as_ref().unwrap(),
            translation.function_body_inputs,
            memory_plans.clone(),
            table_plans.clone(),
        )?;

        // Compile the function call trampolines
        let func_types = translation
            .module
            .signatures
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let function_call_trampolines = compiler
            .compile_function_call_trampolines(&func_types)?
            .into_iter()
            .collect::<PrimaryMap<SignatureIndex, _>>();

        // Compile the dynamic function trampolines
        let dynamic_function_trampolines =
            compiler.compile_dynamic_function_trampolines(&translation.module)?;

        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let target = compiler.target().triple().clone();
        let mut obj = ArtifactBuilder::new(target)
            .name("module".to_string())
            .library(true)
            .finish();
        let function_bodies = compilation.get_function_bodies();

        // We construct the function body lengths
        let function_body_lengths = function_bodies
            .values()
            .map(|function_body| function_body.body.len() as u64)
            .collect::<PrimaryMap<LocalFunctionIndex, u64>>();

        let metadata = ModuleMetadata {
            features: compiler.features().clone(),
            module: Arc::new(translation.module),
            prefix: engine_inner.get_prefix(&data),
            data_initializers,
            function_body_lengths,
            memory_plans,
            table_plans,
        };

        let serialized_data = bincode::serialize(&metadata).map_err(to_compile_error)?;

        obj.declare("WASMER_METADATA", Decl::data().global().read_only())
            .map_err(to_compile_error)?;

        let mut metadata_length = vec![0; 10];
        let mut writable = &mut metadata_length[..];
        leb128::write::unsigned(&mut writable, serialized_data.len() as u64)
            .expect("Should write number");
        metadata_length.extend(serialized_data);
        obj.define("WASMER_METADATA", metadata_length)
            .map_err(to_compile_error)?;

        let function_relocations = compilation.get_relocations();

        // Libcall declarations
        let libcalls = vec![
            "wasmer_f32_ceil",
            "wasmer_f32_floor",
            "wasmer_f32_trunc",
            "wasmer_f32_nearest",
            "wasmer_f64_ceil",
            "wasmer_f64_floor",
            "wasmer_f64_trunc",
            "wasmer_f64_nearest",
            "wasmer_raise_trap",
            "wasmer_probestack",
        ];
        for libcall in libcalls.into_iter() {
            obj.declare(libcall, Decl::function_import())
                .map_err(to_compile_error)?;
            obj.declare(&format!("_{}", libcall), Decl::function_import())
                .map_err(to_compile_error)?;
        }

        // Add functions
        for (function_local_index, function) in function_bodies.into_iter() {
            let function_name = Self::get_function_name(&metadata, function_local_index);
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add function call trampolines
        for (signature_index, function) in function_call_trampolines.into_iter() {
            let function_name = Self::get_function_call_trampoline_name(&metadata, signature_index);
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add dynamic function trampolines
        for (func_index, function) in dynamic_function_trampolines.into_iter() {
            let function_name = Self::get_dynamic_function_trampoline_name(&metadata, func_index);
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add function relocations
        for (function_local_index, function_relocs) in function_relocations.into_iter() {
            let function_name = Self::get_function_name(&metadata, function_local_index);
            for r in function_relocs {
                // assert_eq!(r.addend, 0);
                match r.reloc_target {
                    RelocationTarget::LocalFunc(index) => {
                        let target_name = Self::get_function_name(&metadata, index);
                        // println!("DOING RELOCATION offset: {} addend: {}", r.offset, r.addend);
                        obj.link(Link {
                            from: &function_name,
                            to: &target_name,
                            at: r.offset as u64,
                        })
                        .map_err(to_compile_error)?;
                    }
                    RelocationTarget::LibCall(libcall) => {
                        use wasmer_runtime::libcalls::LibCall;
                        let libcall_fn_name = match libcall {
                            LibCall::CeilF32 => "wasmer_f32_ceil",
                            LibCall::FloorF32 => "wasmer_f32_floor",
                            LibCall::TruncF32 => "wasmer_f32_trunc",
                            LibCall::NearestF32 => "wasmer_f32_nearest",
                            LibCall::CeilF64 => "wasmer_f64_ceil",
                            LibCall::FloorF64 => "wasmer_f64_floor",
                            LibCall::TruncF64 => "wasmer_f64_trunc",
                            LibCall::NearestF64 => "wasmer_f64_nearest",
                            LibCall::RaiseTrap => "wasmer_raise_trap",
                            LibCall::Probestack => "wasmer_probestack",
                        };
                        obj.link(Link {
                            from: &function_name,
                            to: &libcall_fn_name,
                            at: r.offset as u64,
                        })
                        .map_err(to_compile_error)?;
                    }
                    RelocationTarget::CustomSection(_custom_section) => {
                        // TODO: Implement custom sections
                    }
                    RelocationTarget::JumpTable(_func_index, _jt) => {
                        // do nothing
                    }
                };
            }
        }

        let file = NamedTempFile::new().map_err(to_compile_error)?;

        // Re-open it.
        let (file, filepath) = file.keep().map_err(to_compile_error)?;
        obj.write(file).map_err(to_compile_error)?;

        let shared_file = NamedTempFile::new().map_err(to_compile_error)?;
        let (_file, shared_filepath) = shared_file.keep().map_err(to_compile_error)?;
        let output = Command::new("gcc")
            .arg(&filepath)
            .arg("-o")
            .arg(&shared_filepath)
            .arg("-shared")
            .arg("-v")
            .output()
            .map_err(to_compile_error)?;

        if !output.status.success() {
            return Err(CompileError::Codegen(format!(
                "Shared object file generator failed with:\n{}",
                std::str::from_utf8(&output.stderr).unwrap().trim_end()
            )));
        }
        let lib = Library::new(&shared_filepath).map_err(to_compile_error)?;
        Self::from_parts(&mut engine_inner, metadata, shared_filepath, lib)
    }

    fn get_function_name(metadata: &ModuleMetadata, index: LocalFunctionIndex) -> String {
        format!("wasmer_function_{}_{}", metadata.prefix, index.index())
    }

    fn get_function_call_trampoline_name(
        metadata: &ModuleMetadata,
        index: SignatureIndex,
    ) -> String {
        format!(
            "wasmer_trampoline_function_call_{}_{}",
            metadata.prefix,
            index.index()
        )
    }

    fn get_dynamic_function_trampoline_name(
        metadata: &ModuleMetadata,
        index: FunctionIndex,
    ) -> String {
        format!(
            "wasmer_trampoline_dynamic_function_{}_{}",
            metadata.prefix,
            index.index()
        )
    }

    /// Construct a `NativeArtifact` from component parts.
    pub fn from_parts(
        engine_inner: &mut NativeEngineInner,
        metadata: ModuleMetadata,
        sharedobject_path: PathBuf,
        lib: Library,
    ) -> Result<Self, CompileError> {
        let mut finished_functions: PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]> =
            PrimaryMap::new();
        for (function_local_index, function_len) in metadata.function_body_lengths.iter() {
            let function_name = Self::get_function_name(&metadata, function_local_index);
            unsafe {
                // We use a fake funciton signature `fn()` because we just
                // want to get the function address.
                let func: Symbol<unsafe extern "C" fn()> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                let raw = *func.into_raw();
                // The function pointer is a fat pointer, however this information
                // is only used when retrieving the trap information which is not yet
                // implemented in this engine.
                let func_pointer =
                    std::slice::from_raw_parts(raw as *const (), *function_len as usize);
                let func_pointer = std::mem::transmute::<_, *mut [VMFunctionBody]>(func_pointer);
                finished_functions.push(func_pointer);
            }
        }

        // Retrieve function call trampolines (for all signatures in the module)
        for (sig_index, func_type) in metadata.module.signatures.iter() {
            let function_name = Self::get_function_call_trampoline_name(&metadata, sig_index);
            unsafe {
                let trampoline: Symbol<VMTrampoline> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                engine_inner.add_trampoline(&func_type, *trampoline);
            }
        }

        // Retrieve dynamic function trampolines (only for imported functions)
        let mut finished_dynamic_function_trampolines: PrimaryMap<
            FunctionIndex,
            *const VMFunctionBody,
        > = PrimaryMap::with_capacity(metadata.module.num_imported_funcs);
        for func_index in metadata
            .module
            .functions
            .keys()
            .take(metadata.module.num_imported_funcs)
        {
            let function_name = Self::get_dynamic_function_trampoline_name(&metadata, func_index);
            unsafe {
                let trampoline: Symbol<*const VMFunctionBody> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                finished_dynamic_function_trampolines.push(*trampoline);
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
                .module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        Ok(Self {
            sharedobject_path,
            metadata,
            library: lib,
            finished_functions: finished_functions.into_boxed_slice(),
            finished_dynamic_function_trampolines: finished_dynamic_function_trampolines
                .into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
        })
    }

    /// Compile a data buffer into a `NativeArtifact`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(engine: &NativeEngine, data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Serialize a NativeArtifact
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(std::fs::read(&self.sharedobject_path)?)
    }

    /// Deserialize a NativeArtifact
    pub unsafe fn deserialize_from_file(
        engine: &NativeEngine,
        path: &Path,
    ) -> Result<NativeArtifact, DeserializeError> {
        let lib = Library::new(&path).map_err(|e| {
            DeserializeError::CorruptedBinary(format!("Library loading failed: {}", e))
        })?;
        let shared_path: PathBuf = PathBuf::from(path);
        // We use 10 + 1, as the length of the module will take 10 bytes
        // (we construct it like that in `metadata_length`) and we also want
        // to take the first element of the data to construct the slice from
        // it.
        let symbol: Symbol<*mut [u8; 10 + 1]> = lib.get(b"WASMER_METADATA").map_err(|e| {
            DeserializeError::CorruptedBinary(format!("Symbol metadata loading failed: {}", e))
        })?;
        use std::ops::Deref;
        use std::slice;

        let size = &mut **symbol.deref();
        let mut readable = &size[..];
        let metadata_len = leb128::read::unsigned(&mut readable).map_err(|_e| {
            DeserializeError::CorruptedBinary("Can't read metadata size".to_string())
        })?;
        let metadata_slice: &'static [u8] =
            slice::from_raw_parts(&size[10] as *const u8, metadata_len as usize);
        let metadata: ModuleMetadata = bincode::deserialize(metadata_slice)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        let mut engine_inner = engine.inner_mut();

        Self::from_parts(&mut engine_inner, metadata, shared_path, lib)
            .map_err(|e| DeserializeError::Compiler(e))
    }
}

impl Artifact for NativeArtifact {
    fn module(&self) -> &Arc<ModuleInfo> {
        &self.metadata.module
    }

    fn module_ref(&self) -> &ModuleInfo {
        &self.metadata.module
    }

    fn module_mut(&mut self) -> &mut ModuleInfo {
        Arc::get_mut(&mut self.metadata.module).unwrap()
    }

    fn features(&self) -> &Features {
        &self.metadata.features
    }

    fn data_initializers(&self) -> &Box<[OwnedDataInitializer]> {
        &self.metadata.data_initializers
    }

    fn memory_plans(&self) -> &PrimaryMap<MemoryIndex, MemoryPlan> {
        &self.metadata.memory_plans
    }

    fn table_plans(&self) -> &PrimaryMap<TableIndex, TablePlan> {
        &self.metadata.table_plans
    }

    fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]> {
        &self.finished_functions
    }

    fn finished_dynamic_function_trampolines(
        &self,
    ) -> &BoxedSlice<FunctionIndex, *const VMFunctionBody> {
        &self.finished_dynamic_function_trampolines
    }

    fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex> {
        &self.signatures
    }
}
