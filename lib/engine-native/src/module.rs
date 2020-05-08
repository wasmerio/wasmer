//! Define `NativeModule` to allow compiling and instantiating to be
//! done as separate steps.

use crate::engine::{NativeEngine, NativeEngineInner};
use crate::serialize::ModuleMetadata;
use faerie::{ArtifactBuilder, Decl, Link, Reloc, SectionKind};
use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::error::Error;
use std::ffi::c_void;
use std::fmt::Debug;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
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
use wasmer_compiler::{Relocation, RelocationKind, RelocationTarget, Relocations};
use wasmer_engine::{
    resolve_imports, CompiledModule, DeserializeError, Engine, GlobalFrameInfoRegistration,
    InstantiationError, LinkError, Resolver, RuntimeError, SerializableFunctionFrameInfo,
    SerializeError, Tunables,
};
use wasmer_runtime::{
    InstanceHandle, LinearMemory, Module, SignatureRegistry, Table, VMFunctionBody,
    VMGlobalDefinition, VMSharedSignatureIndex, VMTrampoline,
};
use wasmer_runtime::{MemoryPlan, TablePlan};

/// A compiled wasm module, ready to be instantiated.
pub struct NativeModule {
    sharedobject_path: PathBuf,
    metadata: ModuleMetadata,
    library: Library,
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

        let mut metadata_length = vec![0; 10];
        let mut writable = &mut metadata_length[..];
        leb128::write::unsigned(&mut writable, serialized_data.len() as u64)
            .expect("Should write number");
        // println!("metadata length {:?} {}", metadata_length, serialized_data.len());
        metadata_length.extend(serialized_data);
        obj.define("WASMER_METADATA", metadata_length)
            .map_err(to_compile_error)?;

        let function_bodies = compilation.get_function_bodies();
        let function_relocations = compilation.get_relocations();

        obj.declare("wasmer_libcall_f32_ceil", Decl::function_import())
            .map_err(to_compile_error)?;

        for i in 0..metadata.module.num_imported_funcs {
            let imported_function_name = format!("wasmer_imported_function_{}", i);
            obj.declare(imported_function_name, Decl::function_import())
                .map_err(to_compile_error)?;
        }
        // Add functions
        for (function_local_index, function) in function_bodies.iter() {
            let function_name = format!("wasmer_function_{}", function_local_index.index());
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add trampolines
        for (signature_index, function) in trampolines.iter() {
            let function_name = format!("wasmer_trampoline_{}", signature_index.index());
            obj.declare(&function_name, Decl::function().global())
                .map_err(to_compile_error)?;
            obj.define(&function_name, function.body.clone())
                .map_err(to_compile_error)?;
        }

        // Add function relocations
        for (function_local_index, function_relocs) in function_relocations.into_iter() {
            let function_name = format!("wasmer_function_{}", function_local_index.index());
            for r in function_relocs {
                // assert_eq!(r.addend, 0);
                match r.reloc_target {
                    RelocationTarget::LocalFunc(index) => {
                        let target_name = format!("wasmer_function_{}", index.index());
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
                            LibCall::CeilF32 => "wasmer_libcall_f32_ceil",
                            LibCall::FloorF32 => "wasmer_libcall_f32_floor",
                            LibCall::TruncF32 => "wasmer_libcall_f32_trunc",
                            LibCall::NearestF32 => "wasmer_libcall_f32_nearest",
                            LibCall::CeilF64 => "wasmer_libcall_f64_ceil",
                            LibCall::FloorF64 => "wasmer_libcall_f64_floor",
                            LibCall::TruncF64 => "wasmer_libcall_f64_trunc",
                            LibCall::NearestF64 => "wasmer_libcall_f64_nearest",
                            LibCall::RaiseTrap => "wasmer_libcall_raise_trap",
                            LibCall::Probestack => "PROBESTACK",
                            libcall => {
                                unimplemented!("The `{:?}` libcall is not yet implemented", libcall)
                            }
                        };
                        obj.link(Link {
                            from: &function_name,
                            to: &libcall_fn_name,
                            at: r.offset as u64,
                        })
                        .map_err(to_compile_error)?;
                    }
                    RelocationTarget::CustomSection(custom_section) => {}
                    RelocationTarget::JumpTable(func_index, jt) => {
                        // panic!("THERE SHOULDN'T BE JUMP TABLES");
                    }
                };
            }
        }

        let file = NamedTempFile::new().map_err(to_compile_error)?;

        // Re-open it.
        let mut file2 = file.reopen().map_err(to_compile_error)?;
        let (file, filepath) = file.keep().map_err(to_compile_error)?;
        obj.write(file).map_err(to_compile_error)?;

        let shared_file = NamedTempFile::new().map_err(to_compile_error)?;
        let (file, shared_filepath) = shared_file.keep().map_err(to_compile_error)?;
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
                std::str::from_utf8(&output.stderr).unwrap().trim_right()
            )));
        }
        let lib = Library::new(&shared_filepath).map_err(to_compile_error)?;
        Self::from_parts(&mut engine_inner, metadata, shared_filepath, lib)
    }

    /// Construct a `NativeModule` from component parts.
    pub fn from_parts(
        engine_inner: &mut NativeEngineInner,
        metadata: ModuleMetadata,
        sharedobject_path: PathBuf,
        lib: Library,
    ) -> Result<Self, CompileError> {
        let mut finished_functions: PrimaryMap<LocalFunctionIndex, *mut [VMFunctionBody]> =
            PrimaryMap::new();
        for (function_index, function) in metadata
            .module
            .functions
            .iter()
            .skip(metadata.module.num_imported_funcs)
        {
            let function_local_index = metadata.module.local_func_index(function_index).unwrap();
            let function_name = format!("wasmer_function_{}", function_local_index.index());
            unsafe {
                let func: Symbol<unsafe extern "C" fn(i64, i64, i64) -> i64> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                let raw = *func.into_raw();
                // The function pointer is a fat pointer, however this information
                // is only used when retrieving the trap information which is not yet
                // implemented in this backend. That's why se set a lengh 0.
                // TODO: Use the real function length
                let func_pointer = (raw as *const (), 0);
                let func_pointer = std::mem::transmute::<_, *mut [VMFunctionBody]>(func_pointer);
                finished_functions.push(func_pointer);
            }
        }

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

        for (sig_index, _) in metadata.module.signatures.iter() {
            let function_name = format!("wasmer_trampoline_{}", sig_index.index());
            unsafe {
                let trampoline: Symbol<VMTrampoline> = lib
                    .get(function_name.as_bytes())
                    .map_err(to_compile_error)?;
                let func_type = metadata.module.signatures.get(sig_index).unwrap();
                engine_inner.add_trampoline(&func_type, *trampoline);
            }
        }
        // let frame_infos = compilation
        //     .get_frame_info()
        //     .values()
        //     .map(|frame_info| SerializableFunctionFrameInfo::Processed(frame_info.clone()))
        //     .collect::<PrimaryMap<LocalFunctionIndex, _>>();
        // Self::from_parts(&mut engine_inner, lib, metadata, )

        Ok(Self {
            sharedobject_path,
            metadata,
            library: lib,
            finished_functions: finished_functions.into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
        })
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
        Ok(std::fs::read(&self.sharedobject_path)?)
    }

    /// Deserialize a NativeModule
    pub unsafe fn deserialize_from_file(
        engine: &NativeEngine,
        path: &Path,
    ) -> Result<NativeModule, DeserializeError> {
        let lib = Library::new(&path).expect("can't load native file");
        let shared_path: PathBuf = PathBuf::from(path);
        let symbol: Symbol<*mut [u8; 200]> =
            lib.get(b"WASMER_METADATA").expect("Can't get metadata");
        use std::ops::Deref;
        use std::slice;
        use std::slice::from_raw_parts;

        let mut size = &mut **symbol.deref();
        // println!("Size {:?}", size.to_vec());
        let mut readable = &size[..];
        let metadata_len = leb128::read::unsigned(&mut readable).expect("Should read number");
        let metadata_slice: &'static [u8] =
            unsafe { slice::from_raw_parts(&size[10] as *const u8, metadata_len as usize) };
        let metadata: ModuleMetadata = bincode::deserialize(metadata_slice)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        let mut engine_inner = engine.inner_mut();

        Self::from_parts(&mut engine_inner, metadata, shared_path, lib)
            .map_err(|e| DeserializeError::Compiler(e))
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
