//! Define `NativeModule` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{NativeEngine, NativeEngineInner};
use crate::serialize::ModuleMetadata;
use faerie::{ArtifactBuilder, Decl, Link, Reloc, SectionKind};
use libloading::Library;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::error::Error;
use std::ffi::c_void;
use std::fmt::Debug;
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;
use wasm_common::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasm_common::{
    DataInitializer, LocalFunctionIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex,
    MemoryIndex, OwnedDataInitializer, SignatureIndex, TableIndex,
};
use wasmer_compiler::CompileError;
#[cfg(feature = "compiler")]
use wasmer_compiler::ModuleEnvironment;
use wasmer_engine::{
    resolve_imports, CompiledModule, DeserializeError, Engine, GlobalFrameInfoRegistration,
    InstantiationError, LinkError, Resolver, RuntimeError, SerializableFunctionFrameInfo,
    SerializeError, Tunables,
};
use wasmer_runtime::{
    InstanceHandle, LinearMemory, Module, SignatureRegistry, Table, VMFunctionBody,
    VMGlobalDefinition, VMSharedSignatureIndex,
};
use wasmer_runtime::{MemoryPlan, TablePlan};

/// A compiled wasm module, ready to be instantiated.
pub struct NativeModule {
    tempfile_path: PathBuf,
    metadata: ModuleMetadata,
    finished_functions: BoxedSlice<LocalFunctionIndex, *mut [VMFunctionBody]>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
}

type Handle = *mut c_void;

fn to_compile_error(err: impl Error) -> CompileError {
    CompileError::Codegen(format!("{}", err))
}

impl NativeModule {
    /// Compile a data buffer into a `NativeModule`, which may then be instantiated.
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

        // Compile the trampolines
        let func_types = translation
            .module
            .signatures
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let trampolines = compiler
            .compile_wasm_trampolines(&func_types)?
            .into_iter()
            .collect::<PrimaryMap<SignatureIndex, _>>();

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

        // first we declare our symbolic references;
        // it is a runtime error to define a symbol _without_ declaring it first
        // let declarations: Vec<(&'static str, Decl)> = vec![
        //     ("deadbeef", Decl::function().into()),
        //     // ("main", Decl::function().global().into()),
        //     // ("str.1", Decl::cstring().into()),
        //     // ("DEADBEEF", Decl::data_import().into()),
        //     // ("STATIC_REF", Decl::data().global().writable().with_align(Some(64)).into()),
        //     // ("printf", Decl::function_import().into()),
        // ];
        // obj.declarations(declarations.into_iter()).map_err(to_compile_error)?;
        // obj.write(file)?;

        let metadata = ModuleMetadata {
            features: compiler.features().clone(),
            module: Arc::new(translation.module),
            data_initializers,
            memory_plans,
            table_plans,
        };

        let serialized_data = bincode::serialize(&metadata).map_err(to_compile_error)?;

        obj.declare("WASMER_METADATA", Decl::data().global().read_only())
            .map_err(to_compile_error)?;
        obj.define("WASMER_METADATA", serialized_data)
            .map_err(to_compile_error)?;

        let function_bodies = compilation.get_function_bodies();
        let function_relocations = compilation.get_relocations();

        for i in 0..metadata.module.num_imported_funcs {
            let imported_function_name = format!("wasmer_imported_function_{}", i);
            obj.declare(imported_function_name, Decl::function_import())
                .map_err(to_compile_error)?;
        }

        for (function_local_index, function) in function_bodies.iter() {
            let function_name = format!("wasmer_function_{}", function_local_index.index());
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        let file = NamedTempFile::new().map_err(to_compile_error)?;

        // Re-open it.
        let mut file2 = file.reopen().map_err(to_compile_error)?;
        let (file, filepath) = file.keep().map_err(to_compile_error)?;
        obj.write(file).map_err(to_compile_error)?;

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
        let lib = Library::new(&filepath).unwrap();

        Ok(Self {
            tempfile_path: filepath,
            metadata,
            finished_functions: PrimaryMap::new().into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
        })
        // let frame_infos = compilation
        //     .get_frame_info()
        //     .values()
        //     .map(|frame_info| SerializableFunctionFrameInfo::Processed(frame_info.clone()))
        //     .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        // let serializable_compilation = SerializableCompilation {
        //     function_bodies: compilation.get_function_bodies(),
        //     function_relocations: compilation.get_relocations(),
        //     function_jt_offsets: compilation.get_jt_offsets(),
        //     function_frame_info: frame_infos,
        //     trampolines,
        //     custom_sections: compilation.get_custom_sections(),
        // };
        // let serializable = SerializableModule {
        //     compilation: serializable_compilation,
        //     module: Arc::new(translation.module),
        //     features: engine_inner.compiler()?.features().clone(),
        //     data_initializers,
        //     memory_plans,
        //     table_plans,
        // };
        // unimplemented!();
        // Self::from_parts(&mut engine_inner, metadata, )
    }

    /// Compile a data buffer into a `NativeModule`, which may then be instantiated.
    #[cfg(not(feature = "compiler"))]
    pub fn new(engine: &NativeEngine, data: &[u8]) -> Result<Self, CompileError> {
        Err(CompileError::Codegen(
            "Compilation is not enabled in the engine".to_string(),
        ))
    }

    /// Serialize a NativeModule
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        std::fs::read(&self.tempfile_path).map_err(|e| SerializeError::Generic(format!("{:?}", e)))
        // let mut s = flexbuffers::FlexbufferSerializer::new();
        // self.serializable.serialize(&mut s).map_err(|e| SerializeError::Generic(format!("{:?}", e)));
        // Ok(s.take_buffer())
        // unimplemented!("cant' serialize yet");
        // bincode::serialize(&self.serializable)
        //     .map_err(|e| SerializeError::Generic(format!("{:?}", e)))
    }

    /// Deserialize a NativeModule
    pub fn deserialize(
        engine: &NativeEngine,
        bytes: &[u8],
    ) -> Result<NativeModule, DeserializeError> {
        // let r = flexbuffers::Reader::get_root(bytes).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        // let serializable = SerializableModule::deserialize(r).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        unimplemented!();
        // let serializable: SerializableModule = bincode::deserialize(bytes)
        //     .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        // Self::from_parts(&mut engine.inner_mut(), serializable)
        //     .map_err(|e| DeserializeError::Compiler(e))
    }

    /// Construct a `NativeModule` from component parts.
    pub fn from_parts(
        engine_inner: &mut NativeEngineInner,
        metadata: ModuleMetadata,
        dl_handle: Handle,
    ) -> Result<Self, CompileError> {
        unimplemented!();
    }

    fn memory_plans(&self) -> &PrimaryMap<MemoryIndex, MemoryPlan> {
        &self.metadata.memory_plans
    }

    fn table_plans(&self) -> &PrimaryMap<TableIndex, TablePlan> {
        &self.metadata.table_plans
    }

    /// Crate an `Instance` from this `NativeModule`.
    ///
    /// # Unsafety
    ///
    /// See `InstanceHandle::new`
    pub unsafe fn instantiate(
        &self,
        engine: &NativeEngine,
        resolver: &dyn Resolver,
        host_state: Box<dyn Any>,
    ) -> Result<InstanceHandle, InstantiationError> {
        let engine_inner = engine.inner();
        let tunables = engine.tunables();
        let sig_registry: &SignatureRegistry = engine_inner.signatures();
        let imports = resolve_imports(
            &self.module(),
            &sig_registry,
            resolver,
            self.memory_plans(),
            self.table_plans(),
        )
        .map_err(InstantiationError::Link)?;

        let finished_memories = tunables
            .create_memories(&self.module(), self.memory_plans())
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_tables = tunables
            .create_tables(&self.module(), self.table_plans())
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_globals = tunables
            .create_globals(&self.module())
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();

        InstanceHandle::new(
            self.metadata.module.clone(),
            self.finished_functions.clone(),
            finished_memories,
            finished_tables,
            finished_globals,
            imports,
            self.signatures.clone(),
            host_state,
        )
        .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }

    /// Finishes the instantiation of a just created `InstanceHandle`.
    ///
    /// # Unsafety
    ///
    /// See `InstanceHandle::finish_instantiation`
    pub unsafe fn finish_instantiation(
        &self,
        handle: &InstanceHandle,
    ) -> Result<(), InstantiationError> {
        let is_bulk_memory: bool = self.metadata.features.bulk_memory;
        handle
            .finish_instantiation(is_bulk_memory, &self.data_initializers())
            .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }

    /// Returns data initializers to pass to `InstanceHandle::initialize`
    pub fn data_initializers(&self) -> Vec<DataInitializer<'_>> {
        self.metadata
            .data_initializers
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &*init.data,
            })
            .collect::<Vec<_>>()
    }
}

impl CompiledModule for NativeModule {
    fn module(&self) -> &Module {
        &self.metadata.module
    }

    fn module_mut(&mut self) -> &mut Module {
        Arc::get_mut(&mut self.metadata.module).unwrap()
    }
}
