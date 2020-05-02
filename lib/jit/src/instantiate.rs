//! Define the `instantiate` function, which takes a byte array containing an
//! encoded wasm module and returns a live wasm instance. Also, define
//! `CompiledModule` to allow compiling and instantiating to be done as separate
//! steps.

use crate::data::OwnedDataInitializer;
use crate::engine::JITEngineInner;
use crate::error::{DeserializeError, SerializeError};
use crate::error::{InstantiationError, LinkError};
use crate::link::link_module;
use crate::resolver::{resolve_imports, Resolver};
use crate::serialize::{SerializedCompilation, SerializedModule};
use crate::trap::GlobalFrameInfoRegistration;
use crate::trap::RuntimeError;
use crate::trap::{register as register_frame_info, ExtraFunctionInfo};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::{Arc, Mutex};
use wasm_common::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasm_common::{
    DataInitializer, LocalFuncIndex, LocalGlobalIndex, LocalMemoryIndex, LocalTableIndex,
    MemoryIndex, SignatureIndex, TableIndex,
};
use wasmer_compiler::CompileError;
use wasmer_compiler::ModuleEnvironment;
use wasmer_runtime::{
    InstanceHandle, LinearMemory, Module, SignatureRegistry, Table, VMFunctionBody,
    VMGlobalDefinition, VMSharedSignatureIndex,
};

use wasmer_runtime::{MemoryPlan, TablePlan};

/// A compiled wasm module, ready to be instantiated.
pub struct CompiledModule {
    serializable: SerializedModule,

    finished_functions: BoxedSlice<LocalFuncIndex, *mut [VMFunctionBody]>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    frame_info_registration: Mutex<Option<Option<GlobalFrameInfoRegistration>>>,
}

impl CompiledModule {
    /// Compile a data buffer into a `CompiledModule`, which may then be instantiated.
    pub fn new(jit: &mut JITEngineInner, data: &[u8]) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();

        let translation = environ
            .translate(data)
            .map_err(|error| CompileError::Wasm(error))?;

        let memory_plans: PrimaryMap<MemoryIndex, MemoryPlan> = translation
            .module
            .memories
            .iter()
            .map(|(_index, memory_type)| jit.make_memory_plan(memory_type))
            .collect();
        let table_plans: PrimaryMap<TableIndex, TablePlan> = translation
            .module
            .tables
            .iter()
            .map(|(_index, table_type)| jit.make_table_plan(table_type))
            .collect();

        let compilation = jit.compile_module(
            &translation.module,
            translation.module_translation.as_ref().unwrap(),
            translation.function_body_inputs,
            memory_plans.clone(),
            table_plans.clone(),
        )?;
        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let serializable_compilation = SerializedCompilation {
            function_bodies: compilation.get_function_bodies(),
            function_relocations: compilation.get_relocations(),
            function_jt_offsets: compilation.get_jt_offsets(),
            function_frame_info: compilation.get_frame_info(),
        };
        let serializable = SerializedModule {
            compilation: serializable_compilation,
            module: Arc::new(translation.module),
            data_initializers,
            memory_plans,
            table_plans,
        };
        Self::from_parts(jit, serializable)
    }

    /// Serialize a CompiledModule
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        // let mut s = flexbuffers::FlexbufferSerializer::new();
        // self.serializable.serialize(&mut s).map_err(|e| SerializeError::Generic(format!("{:?}", e)));
        // Ok(s.take_buffer())
        bincode::serialize(&self.serializable)
            .map_err(|e| SerializeError::Generic(format!("{:?}", e)))
    }

    /// Deserialize a CompiledModule
    pub fn deserialize(
        jit: &mut JITEngineInner,
        bytes: &[u8],
    ) -> Result<CompiledModule, DeserializeError> {
        // let r = flexbuffers::Reader::get_root(bytes).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        // let serializable = SerializedModule::deserialize(r).map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        let serializable: SerializedModule = bincode::deserialize(bytes)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;

        Self::from_parts(jit, serializable).map_err(|e| DeserializeError::Compiler(e))
    }

    /// Construct a `CompiledModule` from component parts.
    pub fn from_parts(
        jit: &mut JITEngineInner,
        serializable: SerializedModule,
    ) -> Result<Self, CompileError> {
        let finished_functions = jit.compile(
            &serializable.module,
            &serializable.compilation.function_bodies,
        )?;

        link_module(
            &serializable.module,
            &finished_functions,
            &serializable.compilation.function_jt_offsets,
            serializable.compilation.function_relocations.clone(),
        );

        // Compute indices into the shared signature table.
        let signatures = {
            let signature_registry = jit.signatures();
            serializable
                .module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        // Make all code compiled thus far executable.
        jit.publish_compiled_code();

        Ok(Self {
            serializable,
            finished_functions: finished_functions.into_boxed_slice(),
            signatures: signatures.into_boxed_slice(),
            frame_info_registration: Mutex::new(None),
        })
    }

    fn memory_plans(&self) -> &PrimaryMap<MemoryIndex, MemoryPlan> {
        &self.serializable.memory_plans
    }

    fn table_plans(&self) -> &PrimaryMap<TableIndex, TablePlan> {
        &self.serializable.table_plans
    }

    /// Crate an `Instance` from this `CompiledModule`.
    ///
    /// Note that if only one instance of this module is needed, it may be more
    /// efficient to call the top-level `instantiate`, since that avoids copying
    /// the data initializers.
    ///
    /// # Unsafety
    ///
    /// See `InstanceHandle::new`
    pub unsafe fn instantiate(
        &self,
        jit: &JITEngineInner,
        resolver: &dyn Resolver,
        host_state: Box<dyn Any>,
    ) -> Result<InstanceHandle, InstantiationError> {
        let is_bulk_memory: bool = jit.compiler().features().bulk_memory;
        let sig_registry: &SignatureRegistry = jit.signatures();
        let data_initializers = self
            .serializable
            .data_initializers
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &*init.data,
            })
            .collect::<Vec<_>>();
        let imports = resolve_imports(
            &self.serializable.module,
            &sig_registry,
            resolver,
            self.memory_plans(),
            self.table_plans(),
        )
        .map_err(InstantiationError::Link)?;

        let finished_memories = create_memories(&self.serializable.module, self.memory_plans())
            .map_err(InstantiationError::Link)?;
        let finished_tables = create_tables(&self.serializable.module, self.table_plans());
        let finished_globals = create_globals(&self.serializable.module);

        // Register the frame info for the module
        self.register_frame_info();

        InstanceHandle::new(
            self.serializable.module.clone(),
            self.finished_functions.clone(),
            finished_memories,
            finished_tables,
            finished_globals,
            imports,
            &data_initializers,
            self.signatures.clone(),
            is_bulk_memory,
            host_state,
        )
        .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }

    /// Return a reference-counting pointer to a module.
    pub fn module(&self) -> &Arc<Module> {
        &self.serializable.module
    }

    /// Return a reference-counting pointer to a module.
    pub fn module_mut(&mut self) -> &mut Arc<Module> {
        &mut self.serializable.module
    }

    /// Return a reference to a module.
    pub fn module_ref(&self) -> &Module {
        &self.serializable.module
    }

    /// Returns the map of all finished JIT functions compiled for this module
    pub fn finished_functions(&self) -> &BoxedSlice<LocalFuncIndex, *mut [VMFunctionBody]> {
        &self.finished_functions
    }

    /// Register this module's stack frame information into the global scope.
    ///
    /// This is required to ensure that any traps can be properly symbolicated.
    fn register_frame_info(&self) {
        let mut info = self.frame_info_registration.lock().unwrap();
        if info.is_some() {
            return;
        }
        let frame_infos = &self.serializable.compilation.function_frame_info;
        let extra_functions = frame_infos
            .values()
            .map(|frame_info| ExtraFunctionInfo::Processed(frame_info.clone()))
            .collect::<PrimaryMap<LocalFuncIndex, _>>();

        *info = Some(register_frame_info(&self, extra_functions));
    }
}

/// Allocate memory for just the memories of the current module.
fn create_memories(
    module: &Module,
    memory_plans: &PrimaryMap<MemoryIndex, MemoryPlan>,
) -> Result<BoxedSlice<LocalMemoryIndex, LinearMemory>, LinkError> {
    let num_imports = module.num_imported_memories;
    let mut memories: PrimaryMap<LocalMemoryIndex, _> =
        PrimaryMap::with_capacity(module.memories.len() - num_imports);
    for index in num_imports..module.memories.len() {
        let plan = memory_plans[MemoryIndex::new(index)].clone();
        memories.push(LinearMemory::new(&plan).map_err(LinkError::Resource)?);
    }
    Ok(memories.into_boxed_slice())
}

/// Allocate memory for just the tables of the current module.
fn create_tables(
    module: &Module,
    table_plans: &PrimaryMap<TableIndex, TablePlan>,
) -> BoxedSlice<LocalTableIndex, Table> {
    let num_imports = module.num_imported_tables;
    let mut tables: PrimaryMap<LocalTableIndex, _> =
        PrimaryMap::with_capacity(module.tables.len() - num_imports);
    for index in num_imports..module.tables.len() {
        let plan = table_plans[TableIndex::new(index)].clone();
        tables.push(Table::new(&plan));
    }
    tables.into_boxed_slice()
}

/// Allocate memory for just the globals of the current module,
/// with initializers applied.
fn create_globals(module: &Module) -> BoxedSlice<LocalGlobalIndex, VMGlobalDefinition> {
    let num_imports = module.num_imported_globals;
    let mut vmctx_globals = PrimaryMap::with_capacity(module.globals.len() - num_imports);

    for _ in &module.globals.values().as_slice()[num_imports..] {
        vmctx_globals.push(VMGlobalDefinition::new());
    }

    vmctx_globals.into_boxed_slice()
}
