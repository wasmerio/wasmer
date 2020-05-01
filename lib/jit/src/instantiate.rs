//! Define the `instantiate` function, which takes a byte array containing an
//! encoded wasm module and returns a live wasm instance. Also, define
//! `CompiledModule` to allow compiling and instantiating to be done as separate
//! steps.

use crate::engine::JITEngineInner;
use crate::error::{DeserializeError, SerializeError};
use crate::error::{InstantiationError, LinkError};
use crate::link::link_module;
use crate::resolver::{resolve_imports, Resolver};
use crate::serialize::CacheRawCompiledModule;
use crate::trap::GlobalFrameInfoRegistration;
use crate::trap::RuntimeError;
use crate::trap::{register as register_frame_info, ExtraFunctionInfo};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::{Arc, Mutex};
use wasm_common::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasm_common::{
    DataInitializer, DataInitializerLocation, LocalFuncIndex, LocalGlobalIndex, LocalMemoryIndex,
    LocalTableIndex, MemoryIndex, SignatureIndex, TableIndex,
};
use wasmer_compiler::ModuleEnvironment;
use wasmer_compiler::{
    Compilation, CompileError, CompiledFunctionFrameInfo, FunctionAddressMap, TrapInformation,
};
use wasmer_runtime::{
    InstanceHandle, LinearMemory, Module, SignatureRegistry, Table, VMFunctionBody,
    VMGlobalDefinition, VMSharedSignatureIndex,
};

use wasmer_runtime::{MemoryPlan, TablePlan};

/// This is similar to `CompiledModule`, but references the data initializers
/// from the wasm buffer rather than holding its own copy.
struct RawCompiledModule {
    compilation: Compilation,
    module: Module,
    finished_functions: BoxedSlice<LocalFuncIndex, *mut [VMFunctionBody]>,
    data_initializers: Box<[OwnedDataInitializer]>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    traps: BoxedSlice<LocalFuncIndex, Vec<TrapInformation>>,
    address_transform: BoxedSlice<LocalFuncIndex, FunctionAddressMap>,
    // Plans
    memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    table_plans: PrimaryMap<TableIndex, TablePlan>,
}

impl RawCompiledModule {
    /// Create a new `RawCompiledModule` by compiling the wasm module in `data` and instatiating it.
    fn new(jit: &mut JITEngineInner, data: &[u8]) -> Result<Self, CompileError> {
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
        RawCompiledModule::from_parts(
            jit,
            compilation,
            translation.module,
            data_initializers,
            memory_plans,
            table_plans,
        )
    }

    /// Construct a `RawCompiledModule` from the most basic component parts.
    fn from_parts(
        jit: &mut JITEngineInner,
        compilation: Compilation,
        module: Module,
        data_initializers: Box<[OwnedDataInitializer]>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Self, CompileError> {
        let (finished_functions, jt_offsets, relocations, traps, address_transforms) =
            jit.compile(&module, &compilation)?;

        link_module(&module, &finished_functions, &jt_offsets, relocations);

        // Compute indices into the shared signature table.
        let signatures = {
            let signature_registry = jit.signatures();
            module
                .signatures
                .values()
                .map(|sig| signature_registry.register(sig))
                .collect::<PrimaryMap<_, _>>()
        };

        // Make all code compiled thus far executable.
        jit.publish_compiled_code();

        Ok(Self {
            module: module,
            finished_functions: finished_functions.into_boxed_slice(),
            compilation,
            data_initializers: data_initializers,
            signatures: signatures.into_boxed_slice(),
            traps: traps.into_boxed_slice(),
            address_transform: address_transforms.into_boxed_slice(),
            memory_plans,
            table_plans,
        })
    }
}

/// A compiled wasm module, ready to be instantiated.
pub struct CompiledModule {
    compilation: Arc<Compilation>,
    module: Arc<Module>,
    finished_functions: BoxedSlice<LocalFuncIndex, *mut [VMFunctionBody]>,
    data_initializers: Arc<Box<[OwnedDataInitializer]>>,
    signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
    traps: BoxedSlice<LocalFuncIndex, Vec<TrapInformation>>,
    address_transform: BoxedSlice<LocalFuncIndex, FunctionAddressMap>,
    frame_info_registration: Mutex<Option<Option<GlobalFrameInfoRegistration>>>,
    memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
    table_plans: PrimaryMap<TableIndex, TablePlan>,
}

impl CompiledModule {
    /// Compile a data buffer into a `CompiledModule`, which may then be instantiated.
    pub fn new(jit: &mut JITEngineInner, data: &[u8]) -> Result<Self, CompileError> {
        let raw = RawCompiledModule::new(jit, data)?;

        Ok(Self::from_parts(
            raw.compilation,
            raw.module,
            raw.finished_functions,
            raw.data_initializers,
            raw.signatures.clone(),
            raw.traps,
            raw.address_transform,
            raw.memory_plans,
            raw.table_plans,
        ))
    }

    /// Serialize a CompiledModule
    pub fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let cached = CacheRawCompiledModule {
            compilation: self.compilation.clone(),
            module: self.module.clone(),
            data_initializers: self.data_initializers.clone(),
            memory_plans: self.memory_plans.clone(),
            table_plans: self.table_plans.clone(),
        };
        bincode::serialize(&cached).map_err(|e| SerializeError::Generic(format!("{:?}", e)))
    }

    /// Deserialize a CompiledModule
    pub fn deserialize(
        jit: &mut JITEngineInner,
        bytes: &[u8],
    ) -> Result<CompiledModule, DeserializeError> {
        let cached: CacheRawCompiledModule = bincode::deserialize(bytes)
            .map_err(|e| DeserializeError::CorruptedBinary(format!("{:?}", e)))?;
        let compilation = Arc::try_unwrap(cached.compilation);
        let module = Arc::try_unwrap(cached.module);
        let data_initializers = Arc::try_unwrap(cached.data_initializers);
        let memory_plans = cached.memory_plans;
        let table_plans = cached.table_plans;

        if let (Ok(c), Ok(m), Ok(d)) = (compilation, module, data_initializers) {
            let raw = RawCompiledModule::from_parts(jit, c, m, d, memory_plans, table_plans)
                .map_err(|e| DeserializeError::Compiler(e))?;

            return Ok(Self::from_parts(
                raw.compilation,
                raw.module,
                raw.finished_functions,
                raw.data_initializers,
                raw.signatures.clone(),
                raw.traps,
                raw.address_transform,
                raw.memory_plans,
                raw.table_plans,
            ));
        }
        Err(DeserializeError::Generic(
            "Can't deserialize the data".to_string(),
        ))
    }

    /// Construct a `CompiledModule` from component parts.
    pub fn from_parts(
        compilation: Compilation,
        module: Module,
        finished_functions: BoxedSlice<LocalFuncIndex, *mut [VMFunctionBody]>,
        data_initializers: Box<[OwnedDataInitializer]>,
        signatures: BoxedSlice<SignatureIndex, VMSharedSignatureIndex>,
        traps: BoxedSlice<LocalFuncIndex, Vec<TrapInformation>>,
        address_transform: BoxedSlice<LocalFuncIndex, FunctionAddressMap>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Self {
        Self {
            compilation: Arc::new(compilation),
            module: Arc::new(module),
            finished_functions,
            data_initializers: Arc::new(data_initializers),
            signatures,
            traps,
            address_transform,
            frame_info_registration: Mutex::new(None),
            memory_plans,
            table_plans,
        }
    }

    fn memory_plans(&self) -> &PrimaryMap<MemoryIndex, MemoryPlan> {
        &self.memory_plans
    }

    fn table_plans(&self) -> &PrimaryMap<TableIndex, TablePlan> {
        &self.table_plans
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
            .data_initializers
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &*init.data,
            })
            .collect::<Vec<_>>();
        let imports = resolve_imports(
            &self.module,
            &sig_registry,
            resolver,
            self.memory_plans(),
            self.table_plans(),
        )
        .map_err(InstantiationError::Link)?;

        let finished_memories =
            create_memories(&self.module, self.memory_plans()).map_err(InstantiationError::Link)?;
        let finished_tables = create_tables(&self.module, self.table_plans());
        let finished_globals = create_globals(&self.module);

        // Register the frame info for the module
        self.register_frame_info();

        InstanceHandle::new(
            Arc::clone(&self.module),
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
        &self.module
    }

    /// Return a reference-counting pointer to a module.
    pub fn module_mut(&mut self) -> &mut Arc<Module> {
        &mut self.module
    }

    /// Return a reference to a module.
    pub fn module_ref(&self) -> &Module {
        &self.module
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

        let extra_functions = self
            .traps()
            .values()
            .zip(self.address_transform().values())
            .map(|(traps, instrs)| {
                ExtraFunctionInfo::Processed(CompiledFunctionFrameInfo {
                    traps: traps.to_vec(),
                    address_map: (*instrs).clone(),
                })
            })
            .collect::<PrimaryMap<LocalFuncIndex, _>>();

        *info = Some(register_frame_info(&self, extra_functions));
    }

    /// Returns the a map for all traps in this module.
    pub fn traps(&self) -> &BoxedSlice<LocalFuncIndex, Vec<TrapInformation>> {
        &self.traps
    }

    /// Returns a map of compiled addresses back to original bytecode offsets.
    pub fn address_transform(&self) -> &BoxedSlice<LocalFuncIndex, FunctionAddressMap> {
        &self.address_transform
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

/// Similar to `DataInitializer`, but owns its own copy of the data rather
/// than holding a slice of the original module.
#[derive(Clone, Serialize, Deserialize)]
pub struct OwnedDataInitializer {
    /// The location where the initialization is to be performed.
    location: DataInitializerLocation,

    /// The initialization data.
    data: Box<[u8]>,
}

impl OwnedDataInitializer {
    fn new(borrowed: &DataInitializer<'_>) -> Self {
        Self {
            location: borrowed.location.clone(),
            data: borrowed.data.to_vec().into_boxed_slice(),
        }
    }
}
